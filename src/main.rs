use anyhow::bail;
use clokwerk::{Scheduler, TimeUnits};
use futures_util::FutureExt;
use git2::{build::CheckoutBuilder, Repository};
use gotham::{
	handler::assets::FileOptions,
	helpers::http::response::{create_empty_response, create_response},
	hyper::StatusCode,
	router::builder::{build_simple_router, DefineSingleRoute, DrawRoutes},
	state::FromState
};
use gotham_derive::{StateData, StaticResponseExtender};
use log::{error, info};
use once_cell::sync::Lazy;
use s3::{Bucket, Region};
use serde::Deserialize;
use std::{env, path::Path, time::Duration};
use tempfile::tempdir;

const REPO_URL: &str = "https://github.com/maunium/stickerpicker";

fn clone_repo_to<P: AsRef<Path>>(path: P) -> anyhow::Result<Repository> {
	info!("Cloning repository {}", REPO_URL);
	Ok(Repository::clone(REPO_URL, &path)?)
}

fn pull_repo(repo: &Repository) -> anyhow::Result<()> {
	info!("Updating repository");

	repo.remote_set_url("origin", REPO_URL)?;
	repo.find_remote("origin")?.fetch(&["master"], None, None)?;

	let head = repo.find_reference("FETCH_HEAD")?;
	let commit = repo.reference_to_annotated_commit(&head)?;
	let analysis = repo.merge_analysis(&[&commit])?;
	if analysis.0.is_up_to_date() {
	} else if analysis.0.is_fast_forward() {
		let mut reference = repo.find_reference("refs/heads/master")?;
		reference.set_target(commit.id(), "Fast-Forward")?;
		repo.set_head("refs/heads/master")?;
		repo.checkout_head(Some(CheckoutBuilder::default().force()))?;
	} else {
		bail!("Which idiot force-pushed master ???");
	}

	Ok(())
}

#[derive(Deserialize, StateData, StaticResponseExtender)]
struct PathExtractor {
	// This will be a Vec containing each path segment as a separate String, with no '/'s.
	#[serde(rename = "*")]
	parts: Vec<String>
}

static BUCKET: Lazy<Bucket> = Lazy::new(|| {
	let s3_server = env::var("PACKS_S3_SERVER").expect("PACKS_S3_SERVER must be set");
	let s3_bucket = env::var("PACKS_S3_BUCKET").expect("PACKS_S3_BUCKET must be set");
	let region = Region::Custom {
		region: s3_server.clone(),
		endpoint: s3_server
	};
	Bucket::new_public_with_path_style(&s3_bucket, region).expect("Failed to open bucket")
});

fn main() {
	env_logger::init();

	let repo_dir = tempdir().expect("Failed to create tempdir");
	let repo_path = repo_dir.path();
	let repo = clone_repo_to(&repo_path).expect("Failed to download repository");

	let mut scheduler = Scheduler::new();
	scheduler.every(1.hour()).run(move || match pull_repo(&repo) {
		Ok(()) => {},
		Err(e) => error!("Error pulling repository: {}", e)
	});
	let _scheduler_thread = scheduler.watch_thread(Duration::from_secs(60));

	gotham::start(
		"0.0.0.0:8080",
		build_simple_router(move |route| {
			route.get("__ping").to(|state| {
				let res = create_empty_response(&state, StatusCode::NO_CONTENT);
				(state, res)
			});

			route
				.get("/web/packs/*")
				.with_path_extractor::<PathExtractor>()
				.to_async(|mut state| {
					let path = PathExtractor::take_from(&mut state);
					let path = path.parts.join("/");
					info!("Fetching {} from bucket", path);
					async move {
						match BUCKET.get_object(&path).await {
							Ok((data, code)) => {
								info!("Found object {} ({})", path, code);
								let mime = mime_guess::from_path(&path).first().unwrap_or(mime::APPLICATION_OCTET_STREAM);
								let code = StatusCode::from_u16(code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
								let res = create_response(&state, code, mime, data);
								Ok((state, res))
							},
							Err(e) => {
								error!("Error fetching {}: {}", path, e);
								Err((state, e.into()))
							}
						}
					}
					.boxed()
				});

			route.get("/web/*").to_dir(
				FileOptions::new(repo_path.join("web"))
					.with_cache_control("public")
					.with_gzip(true)
					.build()
			);
		})
	);
}
