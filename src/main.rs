use reqwest::blocking::multipart::{Form, Part};
use reqwest::blocking::Client;
use reqwest::header::HeaderMap;
use reqwest::header::HeaderName;
use reqwest::header::HeaderValue;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::{env, fs, path::Path};

#[derive(Deserialize, Clone)]
pub struct ProcessorSettings {
    // Calculation settings
    potion_grouping: u32,
    potion_pricing: f64,
    image_path: String,
    // Listing Info
    title: String,
    description: Vec<String>,
    // Purchased Account Message
    sold_message: Vec<String>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct AccountEntry {
    #[serde(rename = "Username")]
    username: String,
    #[serde(rename = "Password")]
    password: String,
    #[serde(rename = "Cash")]
    cash: u64,
    #[serde(rename = "Age Pots")]
    pots: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EldoradoListing {
    title: String,
    min_potions: u64,
    min_bucks: u64,
    image_path: String,
    description: String,
    accounts: Vec<String>,
    sell_price: String,
}

pub fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        println!("Usage: adoptme-processor <path-to-listing-settings> <path-to-account-data>");
        println!("Example: adoptme-processor settings.json accounts.txt ");
        return Ok(());
    }

    let settings_json = fs::read_to_string(args[1].clone()).expect("File cannot be read");
    let settings: ProcessorSettings = serde_json::from_str(&settings_json).unwrap();

    let mut accounts = Vec::new();

    let mut rdr = csv::Reader::from_path(args[2].clone()).unwrap();
    for result in rdr.deserialize() {
        let record: AccountEntry = result?;
        accounts.push(record);
    }

    let grouped_accounts: Vec<Vec<AccountEntry>> =
        bucket_by_potion_range(&accounts, settings.potion_grouping as u64);

    let mut listings: Vec<EldoradoListing> = Vec::new();
    for (i, group) in grouped_accounts.iter().enumerate() {
        let min_potions = (group.first().unwrap().pots / settings.potion_grouping as u64)
            * settings.potion_grouping as u64;
        let min_bucks = group.iter().map(|a| a.cash).min().unwrap_or(0);
        let max_potions = min_potions + (settings.potion_grouping as u64) - 1;
        let total_accounts = group.len();
        let pots_list: Vec<u64> = group.iter().map(|a| a.pots).collect();
        println!(
            "Group {} ({}-{}): {} accounts -> {:?}",
            i + 1,
            min_potions,
            max_potions,
            total_accounts,
            pots_list
        );

        let title =
            resolve_group_string_with_references("", min_potions, min_bucks, &settings.title);

        let accounts: Vec<String> = group
            .iter()
            .map(|entry| {
                resolve_account_string_with_references(
                    &entry.username,
                    &entry.password,
                    &settings.sold_message.join("\n"),
                )
            })
            .collect();

        listings.push(EldoradoListing {
            description: resolve_group_string_with_references(
                &title,
                min_potions,
                min_bucks,
                &settings.description.join("\n"),
            ),
            image_path: resolve_group_string_with_references(
                &title,
                min_potions,
                min_bucks,
                &settings.image_path,
            ),
            title,
            min_potions,
            accounts,
            sell_price: format!("{:.2}", settings.potion_pricing * min_potions as f64),
            min_bucks,
        })
    }

    fs::write(
        "eldorado.json",
        serde_json::to_string_pretty(&listings).unwrap(),
    )
    .unwrap();

    let template_path =
        env::var("ELDORADO_OFFER_TEMPLATE").unwrap_or_else(|_| "eldorado_make_offer.json".into());

    let listings_from_file: Vec<EldoradoListing> =
        serde_json::from_str(&fs::read_to_string("eldorado.json")?)?;

    if let Err(err) = upload_offers_to_eldorado(&listings_from_file, &template_path) {
        eprintln!("Eldorado upload skipped/failed: {err}");
    }

    let mut income = 0f64;
    for entry in accounts {
        income += entry.pots as f64 * settings.potion_pricing;
    }
    let profit = income * (1f64 - 0.23); // Eldorado cut

    println!("=========Summary========");
    println!("Estimated Gross Income: USD${:.2}", income);
    println!("Estimated Profit: USD${:.2}", profit);
    println!("========================");

    Ok(())
}

fn to_three_digits(n: u64) -> String {
    let mut n = n;
    while n >= 1000 {
        n /= 10;
    }
    format!("{:03}", n)
}

fn resolve_group_string_with_references(
    title: &str,
    min_potion_count: u64,
    min_money_count: u64,
    string: &str,
) -> String {
    string
        .replace("${potions}", &format!("{}", min_potion_count))
        .replace("${bucks}", &format!("{}", to_three_digits(min_money_count)))
        .replace("${title}", title)
}

fn resolve_account_string_with_references(username: &str, password: &str, string: &str) -> String {
    string
        .replace("${username}", &format!("{}", username))
        .replace("${password}", &format!("{}", password))
}

fn bucket_by_potion_range(accounts: &[AccountEntry], group_size: u64) -> Vec<Vec<AccountEntry>> {
    assert!(group_size > 0, "potion_grouping must be > 0");

    // key = bucket start (e.g., 20, 40, 60), value = accounts in that bucket
    let mut buckets: BTreeMap<u64, Vec<AccountEntry>> = BTreeMap::new();

    for acc in accounts {
        let start = (acc.pots / group_size) * group_size; // e.g., 41 / 20 = 2 => 2*20 = 40
        buckets.entry(start).or_default().push(acc.clone());
    }

    // Turn a map into Vec<Vec<AccountEntry>> in ascending bucket order
    buckets.into_iter().map(|(_, v)| v).collect()
}

#[derive(Debug)]
struct UploadedImageInfo {
    small: String,
    large: String,
    original: String,
}

fn upload_offer_image(
    client: &Client,
    headers: &HeaderMap,
    api_base: &str,
    image_upload_path: &str,
    image_path: &str,
) -> Result<UploadedImageInfo, Box<dyn std::error::Error>> {
    let path = Path::new(image_path);
    if !path.exists() {
        return Err(format!("Image not found at path {}", image_path).into());
    }

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("offer-image");

    let part = Part::bytes(fs::read(path)?)
        .file_name(file_name.to_string())
        .mime_str("application/octet-stream")?;

    let form = Form::new().part("file", part); // Eldorado file upload uses "file" field; adjust if API differs.

    let response = client
        .post(format!("{api_base}{image_upload_path}"))
        .headers(headers.clone())
        .multipart(form)
        .send()?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        return Err(format!(
            "Image upload failed with status {} for {}: {}",
            status, image_path, body
        )
        .into());
    }

    let json: Value = response.json()?;
    let paths = json
        .get("localPaths")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            format!(
                "Image upload response missing localPaths for {}: {}",
                image_path,
                serde_json::to_string_pretty(&json).unwrap_or_default()
            )
        })?;

    // Response example: ["/offerimages/...Small.png", "/offerimages/...Large.png", "/offerimages/...Original.png"]
    let mut names = Vec::new();
    for entry in paths {
        if let Some(path_str) = entry.as_str() {
            if let Some(name) = Path::new(path_str).file_name().and_then(|n| n.to_str()) {
                names.push(name.to_string());
            }
        }
    }

    if names.len() < 3 {
        return Err(format!(
            "Unexpected image upload response for {}: {}",
            image_path,
            serde_json::to_string_pretty(&json)?
        )
        .into());
    }

    Ok(UploadedImageInfo {
        small: names[0].clone(),
        large: names[1].clone(),
        original: names[2].clone(),
    })
}

fn upload_offers_to_eldorado(
    listings: &[EldoradoListing],
    template_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let api_key = match env::var("ELDORADO_API_KEY") {
        Ok(value) => value,
        Err(_) => {
            println!(
                "Skipping Eldorado upload: set ELDORADO_API_KEY and other Eldorado config to enable."
            );
            return Ok(());
        }
    };

    let api_base =
        env::var("ELDORADO_API_BASE").unwrap_or_else(|_| "https://www.eldorado.gg".to_string());
    let api_route = env::var("ELDORADO_MAKE_OFFER_PATH")
        .unwrap_or_else(|_| "/api/flexibleOffers/account".to_string());
    let auth_header =
        env::var("ELDORADO_AUTH_HEADER").unwrap_or_else(|_| "Authorization".to_string());
    let auth_scheme = env::var("ELDORADO_AUTH_SCHEME").unwrap_or_else(|_| "Bearer".to_string());
    let guaranteed_delivery =
        env::var("ELDORADO_GUARANTEED_DELIVERY").unwrap_or_else(|_| "Instant".to_string());
    let price_multiplier: f64 = env::var("ELDORADO_PRICE_MULTIPLIER")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100.0); // Default assumes API wants cents; adjust if Eldorado expects a different scale.
    let image_upload_path = env::var("ELDORADO_IMAGE_UPLOAD_PATH")
        .unwrap_or_else(|_| "/api/files/me/Offer".to_string());

    let template = fs::read_to_string(template_path)?;
    let template_json: Value = serde_json::from_str(&template)?;

    let header_name = HeaderName::from_bytes(auth_header.as_bytes())?;
    let header_value = HeaderValue::from_str(&format!("{auth_scheme} {api_key}"))?;
    let mut headers = HeaderMap::new();
    headers.insert(header_name, header_value);

    if let Ok(cookie) = env::var("ELDORADO_COOKIE") {
        headers.insert(
            HeaderName::from_static("cookie"),
            HeaderValue::from_str(&cookie)?,
        );
    }

    let client = Client::new();

    for listing in listings {
        if listing.min_potions < 200 {
            continue;
        }

        let mut payload = template_json.clone();
        set_json_value(
            &mut payload,
            &["details", "offerTitle"],
            Value::String(listing.title.clone()),
        );
        set_json_value(
            &mut payload,
            &["details", "description"],
            Value::String(listing.description.clone()),
        );
        set_json_value(
            &mut payload,
            &["details", "guaranteedDeliveryTime"],
            Value::String(guaranteed_delivery.clone()),
        );

        let quantity = listing.accounts.len() as u64;
        set_json_value(
            &mut payload,
            &["details", "pricing", "quantity"],
            Value::Number(quantity.into()),
        );

        let price = listing
            .sell_price
            .parse::<f64>()
            .map(|p| (p * price_multiplier).round() as u64)
            .unwrap_or(0);
        set_json_value(
            &mut payload,
            &["details", "pricing", "pricePerUnit", "amount"],
            Value::Number(price.into()),
        );

        match upload_offer_image(
            &client,
            &headers,
            &api_base,
            &image_upload_path,
            &listing.image_path,
        ) {
            Ok(img) => {
                set_json_value(
                    &mut payload,
                    &["details", "mainOfferImage", "smallImage"],
                    Value::String(img.small.clone()),
                );
                set_json_value(
                    &mut payload,
                    &["details", "mainOfferImage", "largeImage"],
                    Value::String(img.large.clone()),
                );
                set_json_value(
                    &mut payload,
                    &["details", "mainOfferImage", "originalSizeImage"],
                    Value::String(img.original.clone()),
                );
            }
            Err(err) => {
                return Err(
                    format!("Failed to upload image for '{}': {}", listing.title, err).into(),
                );
            }
        }

        set_json_value(
            &mut payload,
            &["accountSecretDetails"],
            Value::Array(
                listing
                    .accounts
                    .iter()
                    .map(|acc| Value::String(acc.clone()))
                    .collect(),
            ),
        );

        let response = client
            .post(format!("{api_base}{api_route}"))
            .headers(headers.clone())
            .json(&payload)
            .send()?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(format!(
                "Eldorado API returned {} for '{}': {}",
                status, listing.title, body
            )
            .into());
        }

        println!(
            "Uploaded Eldorado offer '{}' ({} accounts, scaled price {})",
            listing.title, quantity, price
        );
    }

    Ok(())
}

fn set_json_value(target: &mut Value, path: &[&str], value: Value) {
    assert!(
        !path.is_empty(),
        "Path must contain at least one element to set a value"
    );

    let mut cursor = target;
    for key in &path[..path.len() - 1] {
        if !cursor.is_object() {
            *cursor = Value::Object(Map::new());
        }

        let map = cursor.as_object_mut().unwrap();
        cursor = map
            .entry((*key).to_string())
            .or_insert_with(|| Value::Object(Map::new()));
    }

    if !cursor.is_object() {
        *cursor = Value::Object(Map::new());
    }

    let map = cursor.as_object_mut().unwrap();
    map.insert(path[path.len() - 1].to_string(), value);
}
