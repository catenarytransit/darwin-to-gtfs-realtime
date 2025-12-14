use anyhow::Result;
use chrono::Utc;
use flate2::read::GzDecoder;
use gtfs_realtime::{FeedHeader, FeedMessage};

use prost::Message;
use quick_xml::de::from_str;
use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use warp::Filter;

mod darwin_types;
mod persistence;
mod processor;
mod state;
mod static_data;

use darwin_types::Pport;
use persistence::{load_state, save_state};
use processor::process_pmap;
use state::AppState;

// GTFS URL provided by Catenary
const GTFS_URL: &str = "https://github.com/catenarytransit/pfaedled-gtfs-actions/releases/download/latest/nationalrailuk.zip";
const DATA_DIR: &str = "./data";

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Initialize State
    println!("Initializing Application State...");
    let state = Arc::new(AppState::new(GTFS_URL.to_string()));

    // 2. Load Persistence (Recovery)
    if let Err(e) = load_state(&state, DATA_DIR) {
        eprintln!("Warning: Failed to load previous state: {}", e);
    }

    // 3. Start GTFS Manager (Background Update)
    if let Err(e) = state.gtfs.load_initial() {
        eprintln!(
            "Warning: Initial GTFS load failed: {}. Background updater will retry.",
            e
        );
    }
    state.gtfs.start_updater();

    // 4. Persistence Loop
    let state_clone_persist = state.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            if let Err(e) = save_state(&state_clone_persist, DATA_DIR) {
                eprintln!("Error saving state: {}", e);
            }
        }
    });

    // 5. HTTP Server
    // Use .boxed() to simplify types
    let state_filter_base = state.clone();
    let state_filter = warp::any().map(move || state_filter_base.clone()).boxed();

    // GET /gtfs-rt
    let gtfs_rt_route = warp::path("gtfs-rt")
        .and(warp::get())
        .and(state_filter.clone())
        .map(|state: Arc<AppState>| {
            let mut msg = FeedMessage::default();
            let mut header = FeedHeader::default();
            header.gtfs_realtime_version = "2.0".to_string();
            header.timestamp = Some(Utc::now().timestamp() as u64);
            msg.header = header;

            for r in state.trip_updates.iter() {
                msg.entity.push(r.value().clone());
            }

            let mut buf = Vec::new();
            msg.encode(&mut buf).unwrap();
            warp::reply::with_header(buf, "content-type", "application/x-protobuf")
        });

    // GET /platforms
    let platforms_route = warp::path("platforms")
        .and(warp::get())
        .and(state_filter.clone())
        .map(|state: Arc<AppState>| {
            let mut data = std::collections::HashMap::new();
            for r in state.platforms.iter() {
                data.insert(r.key().clone(), r.value().clone());
            }
            warp::reply::json(&data)
        });

    // GET /platforms-v2
    let platforms_v2_route = warp::path("platforms-v2")
        .and(warp::get())
        .and(state_filter.clone())
        .map(|state: Arc<AppState>| {
            let mut data = std::collections::HashMap::new();
            for r in state.platforms_v2.iter() {
                data.insert(r.key().clone(), r.value().clone());
            }
            warp::reply::json(&data)
        });

    let routes = gtfs_rt_route
        .or(platforms_route)
        .or(platforms_v2_route)
        .boxed();

    let server_port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| "3000".to_string())
        .parse()
        .expect("Invalid PORT env variable");
    tokio::spawn(warp::serve(routes).run(([0, 0, 0, 0], server_port)));
    println!("Server running at http://localhost:{}", server_port);

    // 6. Connect to Darwin Push Port (Manual STOMP Implementation)
    let username = std::env::var("DARWIN_USER").expect("DARWIN_USER not set");
    let password = std::env::var("DARWIN_PASS").expect("DARWIN_PASS not set");
    let host = std::env::var("DARWIN_HOST")
        .unwrap_or_else(|_| "darwin-dist-44ae45.nationalrail.co.uk".to_string());
    let port_str = std::env::var("DARWIN_PORT").unwrap_or_else(|_| "61613".to_string());
    let port: u16 = port_str.parse().expect("Invalid DARWIN_PORT");
    let destination = "/topic/darwin.pushport-v16";
    let state_clone_stomp = state.clone();

    tokio::spawn(async move {
        loop {
            println!("Connecting to Darwin STOMP at {}:{}...", host, port);
            match connect_and_listen(
                &host,
                port,
                &username,
                &password,
                &destination,
                &state_clone_stomp,
            )
            .await
            {
                Ok(_) => eprintln!("STOMP connection closed unexpectedly."),
                Err(e) => eprintln!("STOMP error: {}", e),
            }
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    });

    loop {
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}

async fn connect_and_listen(
    host: &str,
    port: u16,
    user: &str,
    pass: &str,
    dest: &str,
    state: &AppState,
) -> Result<()> {
    let mut stream = TcpStream::connect((host, port)).await?;
    let (reader, mut writer) = stream.split();
    let mut reader = BufReader::new(reader);

    // 1. Send CONNECT
    let connect_frame = format!(
        "CONNECT\naccept-version:1.2\nlogin:{}\npasscode:{}\n\n\0",
        user, pass
    );
    writer.write_all(connect_frame.as_bytes()).await?;

    // 2. Read CONNECTED
    let _ = read_frame(&mut reader).await?;
    println!("Logged in to Darwin.");

    // 3. Send SUBSCRIBE
    let subscribe_frame = format!(
        "SUBSCRIBE\nid:0\ndestination:{}\nack:client-individual\n\n\0",
        dest
    );
    writer.write_all(subscribe_frame.as_bytes()).await?;

    // 4. Listen Loop
    loop {
        let (headers, frame_body) = read_frame(&mut reader).await?;

        // Process body
        if let Err(e) = process_frame_bytes(&frame_body, state) {
            eprintln!(
                "Error processing frame: {} Body: {}",
                e,
                String::from_utf8_lossy(&frame_body)
            );
        }

        // 5. Send ACK
        // Darwin sends 'ack' header in MESSAGE frame which we must echo back as 'id' in ACK frame?
        // Or standard STOMP says we use 'ack' header from message.
        // Darwin usually provides 'ack' header in the MESSAGE.
        if let Some(ack_id) = headers.get("ack") {
            let ack_frame = format!("ACK\nid:{}\n\n\0", ack_id);
            writer.write_all(ack_frame.as_bytes()).await?;
            // println!("Sent ACK for {}", ack_id);
        } else if let Some(msg_id) = headers.get("message-id") {
            // Some STOMP versions use message-id
            let ack_frame = format!("ACK\nid:{}\n\n\0", msg_id);
            writer.write_all(ack_frame.as_bytes()).await?;
        }
    }
}

// Simple frame reader that returns headers and body
async fn read_frame(
    reader: &mut BufReader<tokio::net::tcp::ReadHalf<'_>>,
) -> Result<(HashMap<String, String>, Vec<u8>)> {
    // 1. Read Command
    let mut command = String::new();
    loop {
        command.clear();
        let bytes = reader.read_line(&mut command).await?;
        if bytes == 0 {
            return Err(anyhow::anyhow!("EOF"));
        }
        if command.trim().is_empty() {
            continue;
        }
        break;
    }

    // 2. Read Headers
    let mut headers = HashMap::new();
    let mut content_length = 0;
    loop {
        let mut header_line = String::new();
        reader.read_line(&mut header_line).await?;
        let trimmed = header_line.trim();
        if trimmed.is_empty() {
            break;
        } // End of headers

        if let Some((k, v)) = trimmed.split_once(':') {
            let key = k.trim().to_lowercase();
            let val = v.trim().to_string();
            if key == "content-length" {
                if let Ok(len) = val.parse::<usize>() {
                    content_length = len;
                }
            }
            headers.insert(key, val);
        }
    }

    // 3. Read Body
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body).await?;

    // 4. Read NULL
    let mut null_byte = [0u8; 1];
    reader.read_exact(&mut null_byte).await?;
    if null_byte[0] != 0 {
        // Warning: Missing null byte or sync error
    }

    Ok((headers, body))
}

fn process_frame_bytes(body: &[u8], state: &AppState) -> Result<()> {
    if body.is_empty() {
        return Ok(());
    }
    // GZip decode
    let mut d = GzDecoder::new(body);
    let mut xml_string = String::new();
    if let Err(_) = d.read_to_string(&mut xml_string) {
        // Maybe not gzipped? Or empty?
        return Ok(());
    }
    // Strip XML namespaces (e.g., ns5:Location -> Location) to satisfy Serde
    let re = regex::Regex::new(r"ns\d+:").unwrap();
    let clean_xml = re.replace_all(&xml_string, "");

    let pport: Pport = match from_str(&clean_xml) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("XML Parsing Error: {}", e);
            eprintln!("Full XML: {}", xml_string); // Log original for debug
            return Err(e.into());
        }
    };
    process_pmap(pport, state);
    Ok(())
}
