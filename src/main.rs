mod utils;
use notify_rust::{Hint, Notification};
use std::{
    collections::HashMap,
    error::Error,
    str::FromStr,
    time::{SystemTime, UNIX_EPOCH},
};

use calamine::{open_workbook, DataType, Ods, RangeDeserializerBuilder, Reader};
use chrono::{Duration, Months, NaiveDateTime};
use clap::Parser;
use colored::Colorize;
use serde::{de::DeserializeOwned, Deserialize};
use std::io::{Read, Seek};

const GRAPH_TOTAL_DAYS: f32 = 365.0;
const GRAPH_CELL_DAYS: f32 = 3.0;
const MIN_BENEFIT: f32 = 10.0;
const UP_TO_DATE_SECONDS: i64 = 14 * 24 * 60 * 60;

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the spreadsheet file with data
    #[arg(short, long)]
    data: String,

    /// Minimal output with Desktop notifications
    #[arg(short, long, default_value_t = false)]
    notifications: bool,
}

#[derive(Deserialize, Debug, PartialEq, Clone, Copy)]
enum DepositStatus {
    Active,
    Closed,
}

#[derive(Deserialize, Debug, PartialEq, Clone, Copy)]
enum PayStrategy {
    Capitalization,
    Once,
}

#[derive(Deserialize, Debug)]
struct Deposit {
    bank: String,
    name: String,
    #[serde(deserialize_with = "parse_date_time")]
    date_open: NaiveDateTime,
    #[serde(deserialize_with = "parse_date_time")]
    date_close: NaiveDateTime,
    amount: f32,
    percent: f32,
    status: DepositStatus,
    pay_strategy: PayStrategy,
}

#[derive(Deserialize, Debug)]
struct Bank {
    name: String,
    percent: f32,
    min_capacity: f32,
    max_capacity: f32,
    transfer_comission: f32,
    pay_strategy: PayStrategy,
}

fn main() {
    let args = Args::parse();
    let res = run_app(&args);
    if let Err(err) = res {
        if args.notifications {
            notify(format!("{}", err.to_string()).as_str());
        } else {
            println!("{:?}", err);
        }
    }
}

fn run_app(args: &Args) -> Result<(), Box<dyn Error>> {
    // let path = format!("{}/samples/data.ods", env!("CARGO_MANIFEST_DIR"));

    let path = &args.data;
    // let path = std::fs::read_to_string(".path")?;
    // println!("File: {}", path.bold());
    let mut doc: Ods<_> = open_workbook(path)?;

    let deposits_own: Vec<Deposit> = read_sheet(&mut doc, "Deposits")?;
    let deposits: Vec<&Deposit> = deposits_own
        .iter()
        .filter(|&dep| dep.status == DepositStatus::Active)
        .collect();
    let banks_own: Vec<Bank> = read_sheet(&mut doc, "Banks")?;
    let banks: Vec<&Bank> = banks_own.iter().collect();

    if args.notifications {
        notify_exists_expired(&args.data)?;
        notify_outdated_data(&deposits)?;
    } else {
        println!();
        println!("{}", "   Graphics   ".bold().black().on_yellow());
        print_deposit_graph(&deposits);

        println!();
        println!("{}", "   Suggestions   ".bold().black().on_yellow());
        print_suggestions(&deposits, &banks);
    }

    Ok(())
}

fn notify(message: &str) {
    Notification::new()
        .summary("Bank Deposit Manager")
        .body(message)
        .hint(Hint::Resident(true))
        .timeout(0)
        .show()
        .unwrap();
}

fn notify_exists_expired(path: &String) -> Result<(), Box<dyn Error>> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64;
    let modified = std::fs::metadata(path)?
        .modified()?
        .duration_since(UNIX_EPOCH)?
        .as_secs() as i64;
    let age = Duration::seconds(now - modified);
    if age.num_seconds() > UP_TO_DATE_SECONDS {
        notify(format!("Data outdated. Last update {} days ago", age.num_days()).as_str());
    }
    Ok(())
}

fn notify_outdated_data(deposits: &Vec<&Deposit>) -> Result<(), Box<dyn Error>> {
    let now = chrono::offset::Local::now().naive_local();
    for dep in deposits {
        let duration = dep.date_close - dep.date_open;
        let opened_ago = now - dep.date_open;
        let close_days = (duration - opened_ago).num_days();
        if close_days < 0 {
            notify("Expired deposits have been found");
        }
    }
    Ok(())
}

/**
Algorithm:

- Calculate total amount across all deposits
- Calculate total amount per bank
- For each deposit:
    - Lookup for all available banks, taking in account diversification bounds (min/max capacity)
    - Choose bank with better percent, taking in account transfer comission
    - Calculate lose, which is currently earned amount.
    - Calculate benefit for the rest of period, if deposit would be reopened
    - If benefit is greater then lose, display suggestion to reopen deposit it that bank
*/
fn print_suggestions(deposits: &Vec<&Deposit>, banks: &Vec<&Bank>) {
    let banks = utils::order_by(banks, |b1, b2| b2.percent.partial_cmp(&b1.percent).unwrap());

    // calculate total amount across all deposits
    let banks_by_name = utils::index_by(&banks, |bank| &bank.name);
    let total_amount = calc_sum_amount(&deposits);
    let deposits_per_bank = utils::group_by(&deposits, |d| &d.bank);
    let total_amount_per_bank: HashMap<_, _> = deposits_per_bank
        .into_iter()
        .map(|(key, bank_deps)| (key, calc_sum_amount(&bank_deps)))
        .collect();
    let now = chrono::offset::Local::now().naive_local();

    let mut lines: Vec<String> = vec![];

    for deposit in deposits {
        let &self_bank = banks_by_name
            .get(&deposit.bank)
            .expect(format!("Unknown bank in deposit {:?}", deposit).as_str());
        // find available banks, already sorted by percent DESC
        let mut best_bank = self_bank;
        let mut transfer_comission: f32 = 0.0; // no comission for self bank
        if check_diversification(
            self_bank,
            -deposit.amount,
            &total_amount_per_bank,
            total_amount,
            true,
            false,
        ) {
            for &bank in banks.iter() {
                if bank.name == self_bank.name {
                    // so self bank is best, stop searching
                    break;
                }
                if !check_diversification(
                    bank,
                    deposit.amount,
                    &total_amount_per_bank,
                    total_amount,
                    false,
                    true,
                ) {
                    continue;
                }
                best_bank = bank;
                transfer_comission = bank.transfer_comission;
                break;
            }
        }

        let comission_amount = deposit.amount * transfer_comission;
        let possible_earn = calc_earn(
            deposit.amount,
            best_bank.percent,
            now,
            deposit.date_close,
            best_bank.pay_strategy,
        ) - comission_amount;
        let current_earn = calc_depo_earn(deposit, deposit.date_close);
        let possible_benefit = possible_earn - current_earn;
        if possible_benefit >= MIN_BENEFIT {
            lines.push(format!("Reopen deposit '{}' ({:.0}k) from {} to {} from {:.2}% to {:.2}% for extra earn {} (including transfer comission {:.2})", 
                deposit.name,
                deposit.amount/1000.0,
                deposit.bank,
                best_bank.name,
                deposit.percent*100.0,
                best_bank.percent*100.0,
                format!("{:.2}", possible_benefit).red().blink(),
                comission_amount,
            ));
        }
    }

    if lines.len() > 0 {
        for line in lines {
            println!("{line}");
        }
    } else {
        println!("{}", "No suggestions".green());
    }
}

fn check_diversification(
    bank: &Bank,
    amount_diff: f32,
    total_amount_per_bank: &HashMap<&String, f32>,
    total_amount: f32,
    check_lower_bound: bool,
    check_upper_bound: bool,
) -> bool {
    let possible_bank_amount = match total_amount_per_bank.get(&bank.name) {
        Some(&total_amount) => total_amount,
        None => 0.0,
    } + amount_diff;
    let possible_bank_capacity = possible_bank_amount / total_amount;
    (!check_lower_bound || bank.min_capacity <= possible_bank_capacity)
        && (!check_upper_bound || possible_bank_capacity <= bank.max_capacity)
}

fn calc_sum_amount(deposits: &Vec<&Deposit>) -> f32 {
    deposits.iter().fold(0.0 as f32, |acc, &e| acc + e.amount)
}

fn print_deposit_graph(deposits: &Vec<&Deposit>) {
    let mut deposits = deposits.clone();
    deposits.sort_by(|&d1, &d2| d2.date_close.cmp(&d1.date_close));

    let mut graph_lines: Vec<String> = vec![];

    let graph_len = GRAPH_TOTAL_DAYS / GRAPH_CELL_DAYS;
    let today_shift = graph_len / 2.0;

    graph_lines.push(format!(
        "{}{} Today",
        " ".repeat(today_shift as usize),
        "V".blue()
    ));
    graph_lines.push("-".repeat(graph_len as usize));

    let now = chrono::offset::Local::now().naive_local();

    let mut total_amount: f32 = 0.0;
    let mut weighted_percent: f32 = 0.0;
    let mut earn_per_day: f32 = 0.0;

    for dep in deposits {
        let earned_now = calc_depo_earn(dep, now);
        let earn_max = calc_depo_earn(dep, dep.date_close);

        let duration = dep.date_close - dep.date_open;
        let duration_days = duration.num_days();
        let opened_ago = now - dep.date_open;
        let close_days = (duration - opened_ago).num_days();
        let close_str = if close_days >= 0 {
            close_days.to_string().green()
        } else {
            close_days.to_string().red().blink()
        };

        graph_lines.push(format!(
            "{}{}",
            " ".repeat(today_shift as usize),
            "|".blue()
        ));
        graph_lines.push(format!(
            "{:5} {:4.0}k for {:5.2}% {:12} close in days: {close_str:4}, duration days: {duration_days:4}, earned {earned_now:5.0} of {earn_max:5.0}",
            dep.bank,
            dep.amount/1000.0,
            dep.percent*100.0,
            ("'".to_owned() + dep.name.as_str() + "'").bold(),
        ));

        let mut bar_shift = today_shift - opened_ago.num_days() as f32 / GRAPH_CELL_DAYS;
        // println!("bar_shift={bar_shift}");
        let mut bar_len = duration_days as f32 / GRAPH_CELL_DAYS;
        if bar_shift < 0.0 {
            bar_len = (bar_len + bar_shift).max(0.0);
            bar_shift = 0.0;
        }
        if bar_len + bar_shift > graph_len {
            bar_len = (graph_len - bar_shift).max(0.0);
        }

        graph_lines.push(format!(
            "{}{}",
            " ".repeat(bar_shift as usize),
            "#".repeat(bar_len as usize).bold().purple().on_purple(),
        ));

        total_amount += dep.amount;
        weighted_percent += dep.amount * dep.percent;
        earn_per_day += earn_max / duration_days as f32;
    }

    graph_lines.push("".to_string());
    graph_lines.push(format!(
        "{} {:.2}k  {} {:.2}%  {} {:.2}k",
        "Sum:".bold(),
        total_amount / 1000.0,
        "Average percent:".bold(),
        100.0
            * if total_amount > 0.0 {
                weighted_percent / total_amount
            } else {
                0.0
            },
        "Monthly earn:".bold(),
        earn_per_day * 30.5 / 1000.0
    ));

    for line in graph_lines {
        println!("{line}");
    }
}

fn calc_depo_earn(deposit: &Deposit, date_end: NaiveDateTime) -> f32 {
    calc_earn(
        deposit.amount,
        deposit.percent,
        deposit.date_open,
        date_end,
        deposit.pay_strategy,
    )
}

fn calc_earn(
    initial_amount: f32,
    percent: f32,
    date_start: NaiveDateTime,
    date_end: NaiveDateTime,
    pay_strategy: PayStrategy,
) -> f32 {
    let percent_per_day = percent / 365.25; // does leap year matter?
    let mut amount = initial_amount;
    let mut date = date_start;
    let mut stop = false;
    let mut total_earn: f32 = 0.0;
    while !stop {
        let mut next_date = date.checked_add_months(Months::new(1)).unwrap();
        if next_date > date_end {
            next_date = date_end;
            stop = true;
        }
        let payable_days = next_date - date;
        let earn = amount * payable_days.num_days() as f32 * percent_per_day;
        if pay_strategy == PayStrategy::Capitalization {
            amount += earn;
        }
        total_earn += earn;
        if stop {
            break;
        }
        date = next_date;
    }
    return total_earn;
}

fn read_sheet<T, R, RS>(doc: &mut R, sheet_name: &str) -> Result<Vec<T>, Box<dyn Error>>
where
    RS: Seek + Read,
    R: Reader<RS>,
    T: DeserializeOwned,
{
    let sheet = doc
        .worksheet_range(sheet_name)
        .ok_or(format!("Can not open sheet {sheet_name}"))?
        .map_err(|err| format!("Failed to parse sheet: {:?}", err))?;

    let mut ret: Vec<T> = Vec::new();
    let mut iter = RangeDeserializerBuilder::new().from_range::<_, T>(&sheet)?;
    while let Some(Ok(row)) = iter.next() {
        ret.push(row);
    }

    Ok(ret)
}

fn parse_date_time<'de, D>(deserializer: D) -> Result<NaiveDateTime, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let data_type = calamine::DataType::deserialize(deserializer)?;
    match &data_type {
        DataType::String(val) => {
            if !val.contains("T") {
                Ok(NaiveDateTime::from_str(&(val.clone() + "T00:00:00")).unwrap())
            } else {
                Ok(NaiveDateTime::from_str(val).unwrap())
            }
        }
        _ => panic!("Invalid DataType for DateTime: {:?}", data_type),
    }
}
