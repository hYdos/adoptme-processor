Rust

## Eldorado Offer Upload
- Set `ELDORADO_API_KEY` to enable automatic POSTing of offers.
- Optional overrides: `ELDORADO_API_BASE` (default `https://www.eldorado.gg`), `ELDORADO_MAKE_OFFER_PATH` (default `/api/flexibleOffers/account`), `ELDORADO_IMAGE_UPLOAD_PATH` (default `/api/files/me/Offer`), `ELDORADO_AUTH_HEADER` (default `Authorization`), `ELDORADO_AUTH_SCHEME` (default `Bearer`), `ELDORADO_GUARANTEED_DELIVERY` (default `Instant`), `ELDORADO_PRICE_MULTIPLIER` (defaults to `100.0` assuming the API expects cents).
- `ELDORADO_OFFER_TEMPLATE` can point to a JSON template (defaults to `eldorado_make_offer.json`); dynamic fields like title, description, quantity, price, main image, and `accountSecretDetails` are filled from `eldorado.json` (each list entry is one offer).
- Upload flow: images at `image_path` are uploaded first to get `mainOfferImage` values, then offers are POSTed using `eldorado.json`. Missing/incorrect endpoints or auth headers must be filled in for your Eldorado account.
- You can pass your raw cookie header via `ELDORADO_COOKIE` if your account requires session cookies in addition to the auth header.
- Run as before: `cargo run -- <listing_settings.json> <account_data.csv>`; `eldorado.json` is produced and then used for uploads when `ELDORADO_API_KEY` is present.
