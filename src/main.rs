use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::{env, fs};

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

#[derive(Serialize, Clone, Debug)]
pub struct EldoradoListing {
    title: String,
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
            accounts,
            sell_price: format!("{:.2}", settings.potion_pricing * min_potions as f64),
        })
    }

    fs::write("eldorado.json", serde_json::to_string_pretty(&listings).unwrap()).unwrap();

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
