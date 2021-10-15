# docker-stickerpicker
Docker container to host https://github.com/maunium/stickerpicker with a self-hosted s3 backend


## Features:
* custom sticker packs for the matrix messenger
* multiple profiles, so different people can use different packs


## Setup Server:

### Requirements:
* (local) s3 bucket, with no password
* Docker
* http reverse proxy for tls (optional, but strongly recommended)

### setup:
docker-compose:
```json
todo
```
Environment variable:
* PACKS_S3_SERVER  the url of your s3 server
* PACKS_S3_BUCKET  the name of your s3 bucket
* HOMESERVER       publicly accessible homeserver url, wihch does render the preview images; Can be different from the server, where the stickers are saved

### add sticker packs:
* create a stickerpack using the [stickerpicker-import/creating script](https://github.com/maunium/stickerpicker/wiki/Creating-packs)
* upload the created `.json` of your stickerpack (located in `web/packs/`) to `/PROFILE_NAME/*.json` at your s3 bucket. 
  ⚠️ **Do not upload the `index.json`**. The server creates this file.


## Settup Client:
* enter `/devtools` in a chat in element.
* go to: Explore Account Datat -> m.widgets
* change the json to:
```json
{
	"stickerpicker": {
		"content": {
			"type": "m.stickerpicker",
			"url": "https://YOUR.STICKER.PICKER.URL/PROFILE_NAME/index.html?theme=$theme",
			"name": "Stickerpaket",
			"data": {}
		},
		"sender": "@YOU:MATRIX.SERVER.NAME",
		"state_key": "stickerpicker",
		"type": "m.widget",
		"id": "stickerpicker"
	}
}
```
Do not forget to change `YOUR.STICKER.PICKER.URL/PROFILE_NAME`, `PROFILE_NAME` and `@YOU:MATRIX.SERVER.NAME`
