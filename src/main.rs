/*
 * Copyright (c) 2018 Boucher, Antoni <bouanto@zoho.com>
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy of
 * this software and associated documentation files (the "Software"), to deal in
 * the Software without restriction, including without limitation the rights to
 * use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of
 * the Software, and to permit persons to whom the Software is furnished to do so,
 * subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS
 * FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR
 * COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER
 * IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN
 * CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
 */

extern crate alsa;
//extern crate json;
extern crate nix;
//extern crate password_store;
extern crate rem;
extern crate time;
//extern crate tls_api;
//extern crate tls_api_openssl;

use std::env::home_dir;
use std::fs::{File, read_dir};
//use std::io::{Read, Write};
use std::io::Read;
//use std::net::TcpStream;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::thread;
use std::time::Duration;

use alsa::mixer::{Mixer, SelemChannelId, SelemId};
//use json::parse;
use nix::ifaddrs::getifaddrs;
use nix::sys::socket::{InetAddr, SockAddr};
//use password_store::PasswordStore;
use time::Tm;
//use tls_api::{TlsConnector, TlsConnectorBuilder};
//use tls_api_openssl::TlsConnector as Connector;

const GREEN: &str = "#00FF00";
const PURPLE: &str = "#FF00FF";
const RED: &str = "#FF0000";
const TURQUOISE: &str = "#00FFFF";
const YELLOW: &str = "#FFFF00";

const MONTHS: &[&str] = &["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre",
    "octobre", "novembre", "décembre"];

const EXCLUDE_MAILBOXES: &[&str] = &["Adgear"];
const OFFLINEIMAP_CONFIG_DIR: &str = ".config/offlineimap/";
const REMIND_CONFIG_DIR: &str = ".config/remind/";

const SUMMARY_LEN: usize = 15;

static INTERNET_USAGE_VALUE: AtomicIsize = AtomicIsize::new(-1);

fn main() {
    /*thread::spawn(|| {
        let internet_usage = (|| {
            let tcp_stream = TcpStream::connect("www.videotron.com:443").ok()?;
            let mut stream = Connector::builder().ok()?.build().ok()?
                .connect("www.videotron.com", tcp_stream).ok()?;
            let caller = "antoyo.videotron.ca";
            let userkey = PasswordStore::get("videotron/userkey").ok()?;
            stream.write(format!("GET /api/1.0/internet/usage/wired/{}.json?caller={} HTTP/1.1\r\nHost: www.videotron.com\r\n\r\n", userkey, caller).as_bytes()).ok()?;
            let mut string = String::new();
            stream.read_to_string(&mut string).ok()?;
            let mut parts = string.split("\r\n\r\n");
            parts.next(); // Skip headers.
            let body = parts.next()?;
            let data = parse(body).ok()?;
            let internet_usage: f64 = data["internetAccounts"][0]["combinedPercent"].as_str()?.parse().ok()?;
            Some(internet_usage)
        })();
        if let Some(internet_usage) = internet_usage {
            INTERNET_USAGE_VALUE.store(internet_usage.round() as isize, Ordering::SeqCst);
        }
    });*/

    println!(r#"{{"version": 1}}"#);
    println!("[");
    println!("[],");

    loop {
        let mut entries = Entries(vec![]);
        entries.add_many(calendar_entries());
        entries.add_many(mail_entries());
        entries.add(internet_usage_entry());
        entries.add(ip_entry());
        entries.add(volume_entry());
        entries.add(battery_entry());
        entries.add(datetime_entry());

        println!("[");
        let entries = entries.0.iter()
            .map(Entry::to_json)
            .collect::<Vec<_>>()
            .join(",\n");
        println!("{}", entries);
        println!("],");

        thread::sleep(Duration::from_secs(1));
    }
}

struct Entries(Vec<Entry>);

impl Entries {
    fn add<E: Into<Option<Entry>>>(&mut self, entry: E) {
        if let Some(entry) = entry.into() {
            self.0.push(entry);
        }
    }

    fn add_many(&mut self, entry: Vec<Entry>) {
        self.0.extend(entry);
    }
}

fn battery_entry() -> Option<Entry> {
    let energy = read_u64("/sys/class/power_supply/BAT0/energy_now")?;
    let power = read_u64("/sys/class/power_supply/BAT0/power_now")?;

    if power == 0 {
        return None;
    }

    let mut status_file = File::open("/sys/class/power_supply/BAT0/status").ok()?;
    let mut status = String::new();
    status_file.read_to_string(&mut status).ok()?;
    status.pop(); // Remove newline.

    match status.as_str() {
        "Discharging" => {
            let time = energy as f64 / power as f64;
            let minutes = (time * 60.0) as u64;
            let hours = minutes / 60;
            let minutes = minutes % 60;
            Some(Entry::new("battery", format!("{}:{:02}", hours, minutes)))
        },
        // TODO: other state.
        _ => None,
    }
}

fn calendar_entries() -> Vec<Entry> {
    let mut result = vec![];
    let entries = (|| {
        let remind_filename = home_dir()?.join(REMIND_CONFIG_DIR).join("reminders.rem");
        let file = File::open(remind_filename).ok()?;
        rem::parse(file).ok()
    })();
    if let Some(entries) = entries {
        let mut sorted_events = vec![];
        for entry in entries {
            let now = time::now();
            let entry_date = Tm {
                tm_sec: 0,
                tm_min: entry.time.minute as i32,
                tm_hour: entry.time.hour as i32,
                tm_mday: entry.date.day as i32,
                tm_mon: entry.date.month as i32,
                tm_year: entry.date.year as i32 - 1900,
                tm_wday: 0,
                tm_yday: 0,
                tm_isdst: 0,
                tm_utcoff: -5,
                tm_nsec: 0,
            };
            let time_delta = entry_date - now;
            if time_delta <= time::Duration::days(7) && time_delta >= time::Duration::days(0) {
                sorted_events.push(entry);
            }
        }
        sorted_events.sort_by_key(|entry| (entry.date, entry.time));
        sorted_events.reverse();
        for entry in sorted_events {
            let words = entry.msg.split_whitespace();
            let mut summary = String::new();
            for word in words {
                if summary.len() > SUMMARY_LEN {
                    break;
                }
                summary.push(' ');
                summary.push_str(word);
            }
            result.push(Entry::new_colored(&format!("event_{}-{}", entry.date.day, entry.date.month as u8),
                format!("{} {}: {}", entry.date.day, MONTHS[entry.date.month as usize], summary), PURPLE));
        }
    }
    result
}

fn datetime_entry() -> Option<Entry> {
    const DAYS: &[&str] = &["dimanche", "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi"];

    let now = time::now();
    let format = format!("{} {} {} {} | {}:{:02}", DAYS[now.tm_wday as usize], now.tm_mday, MONTHS[now.tm_mon as usize],
        now.tm_year + 1900, now.tm_hour, now.tm_min);
    Some(Entry::new("datetime", format))
}

fn internet_usage_entry() -> Option<Entry> {
    let internet_usage = INTERNET_USAGE_VALUE.load(Ordering::SeqCst);
    if internet_usage == -1 {
        // Not loaded yet.
        return None;
    }
    if internet_usage > 85 {
        Some(Entry::new_colored("internet_usage", format!("⇵ {}%", internet_usage), RED))
    }
    else {
        Some(Entry::new("internet_usage", format!("⇵ {}%", internet_usage)))
    }
}

fn ip_entry() -> Option<Entry> {
    let mut ethernet_address = None;
    let mut wifi_address = None;
    for addr in getifaddrs().ok()? {
        if let Some(address) = addr.address {
            let address =
                if let SockAddr::Inet(address@InetAddr::V4(_)) = address {
                    address.to_std().ip().to_string()
                }
                else {
                    continue;
                };
            match addr.interface_name.chars().next() {
                Some('w') => wifi_address = Some(address),
                Some('e') => ethernet_address = Some(address),
                _ => (),
            }
        }
    }
    let entry =
        match (ethernet_address, wifi_address) {
            (_, Some(address)) => Entry::new_colored("network", format!("W: {}", address), GREEN),
            (Some(address), None) => Entry::new_colored("network", format!("E: {}", address), GREEN),
            (None, None) => Entry::new_colored("network", "No network".to_string(), RED),
        };
    Some(entry)
}

fn mail_entries() -> Vec<Entry> {
    let mut entries = vec![];
    (|| {
        let offlineimap_dir = home_dir()?.join(OFFLINEIMAP_CONFIG_DIR);
        for file in read_dir(&offlineimap_dir).ok()? {
            let file = file.ok()?;
            if file.file_type().ok()?.is_dir() {
                let filename = file.file_name();
                let mailbox = filename.to_str()?;
                if !EXCLUDE_MAILBOXES.contains(&mailbox) {
                    let mailbox_dir = offlineimap_dir.join(mailbox);
                    let mut unread_mail_count = 0;
                    for file in read_dir(&mailbox_dir).ok()? {
                        let file = file.ok()?;
                        let new_dir = mailbox_dir.join(file.file_name()).join("new");
                        unread_mail_count += read_dir(new_dir).ok()?.count();
                    }
                    if unread_mail_count > 0 {
                        let name = format!("{}_email", mailbox);
                        entries.push(Entry::new_colored(&name, format!("✉ {} ({})", mailbox, unread_mail_count),
                        TURQUOISE));
                    }
                }
            }
        }
        Some(())
    })();
    entries
}

fn volume_entry() -> Option<Entry> {
    let mixer = Mixer::new("default", false).ok()?;
    let selem_id = SelemId::new("Master", 0);
    let selem = mixer.find_selem(&selem_id)?;
    let channel_id = SelemChannelId::mono();
    let volume = selem.get_playback_volume(channel_id).ok()?;
    let (volume_min, volume_max) = selem.get_playback_volume_range();
    let volume_percent = ((volume - volume_min) as f64 / volume_max as f64 * 100.0).round() as i32;
    let muted = selem.get_playback_switch(channel_id).ok()? == 0;
    let entry =
        if muted {
            Entry::new_colored("volume", "♪: 0%".to_string(), YELLOW)
        }
        else {
            Entry::new("volume", format!("☊ {}%", volume_percent))
        };
    Some(entry)
}

struct Entry {
    color: Option<&'static str>,
    name: String,
    full_text: String,
}

impl Entry {
    fn new(name: &str, full_text: String) -> Self {
        Self {
            color: None,
            name: name.to_string(),
            full_text,
        }
    }

    fn new_colored(name: &str, full_text: String, color: &'static str) -> Self {
        Self {
            color: Some(color),
            name: name.to_string(),
            full_text,
        }
    }

    fn to_json(&self) -> String {
        let color =
            if let Some(color) = self.color {
                format!("\n    \"color\": {:?},", color)
            }
            else {
                String::new()
            };
        let full_text = self.full_text.replace("\"", "\\\"");
        format!(r#"{{{}
    "name": {:?},
    "full_text": "{}"
}}"#, color, self.name, full_text)
    }
}

fn read_u64(path: &str) -> Option<u64> {
    let mut file = File::open(path).ok()?;
    let mut buffer = String::new();
    file.read_to_string(&mut buffer).ok()?;
    buffer.pop(); // Remove newline.
    buffer.parse().ok()
}
