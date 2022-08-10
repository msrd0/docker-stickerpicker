use anyhow::bail;
use clokwerk::{Scheduler, TimeUnits};
use futures_util::FutureExt;
use git2::{build::CheckoutBuilder, Repository};
use gotham::{
	handler::FileOptions,
	helpers::http::response::{create_empty_response, create_response},
	hyper::{
		header::{HeaderValue, LOCATION},
		StatusCode
	},
	mime::{APPLICATION_JSON, APPLICATION_OCTET_STREAM},
	prelude::*,
	router::builder::build_simple_router
};
use log::{error, info};
use once_cell::sync::Lazy;
use s3::{request_trait::ResponseData, Bucket, Region};
use serde::{Deserialize, Serialize};
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

#[derive(Deserialize, StateData, StaticResponseExtender)]
struct ProfileExtractor {
	profile: String
}

static BUCKET: Lazy<Bucket> = Lazy::new(|| {
	let s3_server =
		env::var("PACKS_S3_SERVER").expect("PACKS_S3_SERVER must be set");
	let s3_bucket =
		env::var("PACKS_S3_BUCKET").expect("PACKS_S3_BUCKET must be set");
	let region = Region::Custom {
		region: s3_server.clone(),
		endpoint: s3_server
	};
	Bucket::new_public(&s3_bucket, region)
		.expect("Failed to open bucket")
		.with_path_style()
});

static HOMESERVER: Lazy<String> =
	Lazy::new(|| env::var("HOMESERVER").expect("HOMESERVER must be set"));

#[derive(Serialize)]
struct Index<'a> {
	packs: Vec<String>,
	homeserver_url: &'a str
}

fn main() {
	env_logger::init();

	let repo_dir = tempdir().expect("Failed to create tempdir");
	let repo_path = repo_dir.path();
	let repo = clone_repo_to(&repo_path).expect("Failed to download repository");

	let mut scheduler = Scheduler::new();
	scheduler
		.every(1.hour())
		.run(move || match pull_repo(&repo) {
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
				.get("/:profile/packs/index.json")
				.with_path_extractor::<ProfileExtractor>()
				.to_async(|mut state| {
					let path: ProfileExtractor = state.take();
					async move {
						match BUCKET
							.list(
								format!("/{}/", path.profile),
								Some("/".to_owned())
							)
							.await
						{
							Ok(list) => {
								let mut packs = list
									.into_iter()
									.flat_map(|chunk| chunk.contents.into_iter())
									.map(|obj| obj.key)
									.filter(|key| key.ends_with(".json"))
									.collect::<Vec<_>>();
								packs.sort_unstable();
								let index = Index {
									packs,
									homeserver_url: &HOMESERVER
								};
								let json = serde_json::to_vec(&index).unwrap();
								let res = create_response(
									&state,
									StatusCode::OK,
									APPLICATION_JSON,
									json
								);
								Ok((state, res))
							},
							Err(e) => {
								error!("Error listing bucket: {}", e);
								Err((state, e.into()))
							}
						}
					}
					.boxed()
				});

			route
				.get("/:profile/packs/*")
				.with_path_extractor::<PathExtractor>()
				.to_async(|mut state| {
					let path = PathExtractor::take_from(&mut state);
					let path = path.parts.join("/");
					info!("Fetching {} from bucket", path);
					async move {
						match BUCKET.get_object(&path).await {
							Ok(data) => {
								info!(
									"Found object {} ({})",
									path,
									data.status_code()
								);
								let mime = mime_guess::from_path(&path)
									.first()
									.unwrap_or(APPLICATION_OCTET_STREAM);
								let code = StatusCode::from_u16(data.status_code())
									.unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
								let res = create_response(
									&state,
									code,
									mime,
									<ResponseData as Into<Vec<u8>>>::into(data)
								);
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

			route.get("/:profile/*").to_dir(
				FileOptions::new(repo_path.join("web"))
					.with_cache_control("public")
					.with_gzip(true)
					.build()
			);

			route.get("/").to(|state| {
				let mut res =
					create_empty_response(&state, StatusCode::PERMANENT_REDIRECT);
				res.headers_mut()
					.insert(LOCATION, HeaderValue::from_static("/web/index.html"));
				(state, res)
			})
		})
	)
	.expect("Failed to start gotham server");
}
