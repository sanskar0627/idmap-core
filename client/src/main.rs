mod client;
use anyhow::Result;

/// 🔹 Entry point (only calls run_client)
#[tokio::main]
async fn main() -> Result<()> {
    client::run_client().await
}

// use dkg_tcp::{TcpIncoming, config, keygen, sign};
// use futures::StreamExt;
// use hex;
// use redis::Commands;
// use serde::{Deserialize, Serialize};
// use sqlx::{Pool, Postgres, Row};
// use std::sync::Arc;
// use tokio::net::TcpStream;

// #[derive(Serialize, Deserialize, Debug, Clone)]
// pub struct MessageToSign {
//     pub data: Vec<u8>,
// }

// #[tokio::main]
// async fn main() -> Result<()> {
//     let id: u64 = 1;

//     let config = config::Config::load();
//     println!("[CLIENT] using redis: {}", config.redis_url);

//     let db_pool: Pool<Postgres> = config::get_db_pool().await?;
//     println!("[SERVER] Connected to PostgreSQL");

//     let socket = TcpStream::connect("127.0.0.1:7000").await?;

//     let std_stream = socket.into_std()?;
//     std_stream.set_nonblocking(true)?;
//     let std_stream_dkg = std_stream.try_clone()?;
//     let std_stream_sign = std_stream.try_clone()?;
//     let std_stream_message = std_stream.try_clone()?;

//     let reader_stream_message = TcpStream::from_std(std_stream_message.try_clone()?)?;

//     // ========== DKG PHASE ==========
//     let session = b"session-001"; // make it dynamic
//     let valid_shares = keygen::generate_private_share(std_stream_dkg, id, session).await?;

//     let pubkey_bytes = valid_shares.shared_public_key().to_bytes(true);
//     let solana_address = bs58::encode(pubkey_bytes).into_string();
//     println!("SOLANA ADDRESS: {}", solana_address);
//     println!(
//         "shared public key (hex): {}",
//         hex::encode(valid_shares.shared_public_key().to_bytes(true))
//     );
//     println!("finished DKG for server");

//     // store the encrypted share in the postgres database for now
//     let encrypted_share: Vec<u8> = bincode::serialize(&valid_shares)?.to_vec(); // TODO: encrypt it 
//     sqlx::query(
//         "INSERT INTO share_schema.shares (nodeId, sessionId, solanaAddress, encryptedShare)
//      VALUES ($1, $2, $3, $4)",
//     )
//     .bind(id as i64)
//     .bind(session)
//     .bind(&solana_address)
//     .bind(&encrypted_share)
//     .execute(&db_pool)
//     .await?;

//     println!("[CLIENT] Stored encrypted DKG share in Postgres");

//     // ========== SIGNING PHASE ==========
//     let mut incoming_message = TcpIncoming::<MessageToSign>::new(reader_stream_message, id);
//     let mut data_to_sign: Option<Vec<u8>> = None;

//     // get the message to sign from the other server
//     while let Some(result) = incoming_message.next().await {
//         match result {
//             Ok(msg) => {
//                 println!("Received {} bytes to sign", msg.msg.data.len());
//                 data_to_sign = Some(msg.msg.data); // store it
//                 break; // exit after receiving one message
//             }
//             Err(e) => {
//                 eprintln!("Error receiving message: {:?}", e);
//                 break; // stop trying if error
//             }
//         }
//     }

//     let data_to_sign = match data_to_sign {
//         Some(data) => data,
//         None => {
//             eprintln!("No data received to sign!");
//             return Ok(());
//         }
//     };

//     // get the share from the database and decrypt that
//     let row: sqlx::postgres::PgRow = sqlx::query(
//         "SELECT encryptedShare FROM share_schema.shares WHERE nodeId = $1 AND sessionId = $2",
//     )
//     .bind(id as i64)
//     .bind(session)
//     .fetch_one(&db_pool)
//     .await?;

//     let encrypted_share: Vec<u8> = row.try_get("encryptedShare")?;
//     let valid_shares = bincode::deserialize(&encrypted_share)?;

//     println!("[CLIENT] Retrieved and decrypted share from DB");

//     // start signing protocol
//     let (r_slice, z_slice) =
//         sign::run_signing_phase(id, valid_shares, std_stream_sign, data_to_sign).await?;

//     println!("Signature created successfully! [client]");
//     println!("r: {}", hex::encode(r_slice));
//     println!("z: {}", hex::encode(z_slice));

//     Ok(())
// }

// // run dkg and sign concurrently

// async fn run_dkg_client(redis_client: Arc<redis::Client>, id: u64) -> Result<()> {
//     let mut pub_conn = redis_client.get_connection()?;
//     let mut sub_conn = redis_client.get_connection()?;
//     let mut pubsub = sub_conn.as_pubsub();
//     pubsub.subscribe("dkg-start")?;

//     let db_pool: Pool<Postgres> = config::get_db_pool().await?;

//     println!("[CLIENT-DKG] Listening on Redis channel `dkg-start`");

//     // TODO: update the port
//     let socket = TcpStream::connect("127.0.0.1:7001").await?;
//     let std_stream = socket.into_std()?;
//     std_stream.set_nonblocking(true)?;

//     loop {
//         let msg = pubsub.get_message()?;
//         let payload: String = msg.get_payload()?;
//         println!("[CLIENT-DKG] Received Redis msg: {}", payload);

//         let parsed: serde_json::Value = serde_json::from_str(&payload)?;

//         // TODO: validate the payload

//         let session = parsed["session"].clone().to_string();

//         if parsed["action"] == "startdkg" {
//             println!("[CLIENT-DKG] Starting key generation...");

//             let valid_shares =
//                 keygen::generate_private_share(std_stream.try_clone()?, id, session.as_bytes())
//                     .await?;

//             let pubkey_bytes = valid_shares.shared_public_key().to_bytes(true);
//             let sol_address = bs58::encode(pubkey_bytes).into_string();

//             println!("[CLIENT-DKG] Solana Address: {}", sol_address);

//             // store the encrypted share in the postgres database for now: let encrypted_share: Vec<u8> = bincode::serialize(&valid_shares)?.to_vec(); // TODO: encrypt it
//             let encrypted_share: Vec<u8> = bincode::serialize(&valid_shares)?.to_vec(); // TODO: encrypt it 
//             sqlx::query(
//                 "INSERT INTO share_schema.shares (nodeId, sessionId, solanaAddress, encryptedShare)
//                     VALUES ($1, $2, $3, $4)",
//             )
//             .bind(id as i64)
//             .bind(session)
//             .bind(&sol_address)
//             .bind(&encrypted_share)
//             .execute(&db_pool)
//             .await?;

//             let result = serde_json::json!({
//                 "id": parsed["id"],
//                 "result_type": "dkg-result",
//                 "data": sol_address
//             });
//             let _: () = pub_conn.publish("dkg-result", result.to_string())?;
//             println!("[CLIENT-DKG] Published DKG result to Redis!");
//         }
//     }
// }

// async fn run_sign_client(redis_client: Arc<redis::Client>, id: u64) -> Result<()> {
//     let mut pub_conn = redis_client.get_connection()?;
//     let mut sub_conn = redis_client.get_connection()?;
//     let mut pubsub = sub_conn.as_pubsub();
//     pubsub.subscribe("sign-start")?;
//     println!("[CLIENT-SIGN] Listening on Redis channel `sign-start`");

//     let db_pool: Pool<Postgres> = config::get_db_pool().await?;

//     // TODO: connect only once, and use that connection in every subsequent request
//     let socket = TcpStream::connect("127.0.0.1:7002").await?; // TODO: update the port
//     let std_stream = socket.into_std()?;
//     std_stream.set_nonblocking(true)?;

//     loop {
//         let msg = pubsub.get_message()?;
//         let payload: String = msg.get_payload()?;
//         println!("[CLIENT-SIGN] Received Redis msg: {}", payload);

//         let parsed: serde_json::Value = serde_json::from_str(&payload)?;
//         let session = parsed["session"].clone();

//         // TODO: verify and validate parsed data

//         if parsed["action"] == "sign" {
//             println!("[CLIENT-SIGN] Starting signing phase...");

//             // replace this depreciated function
//             let message_bytes = base64::decode(parsed["message"].as_str().unwrap())?;

//             // fetch the
//             let row: sqlx::postgres::PgRow = sqlx::query(
//                 "SELECT encryptedShare FROM share_schema.shares WHERE nodeId = $1 AND sessionId = $2",
//                 )
//                 .bind(id as i64)
//                 .bind(session)
//                 .fetch_one(&db_pool)
//                 .await?;

//             let encrypted_share: Vec<u8> = row.try_get("encryptedShare")?;
//             let valid_shares = bincode::deserialize(&encrypted_share)?;

//             let (r, z) =
//                 sign::run_signing_phase(id, valid_shares, std_stream.try_clone()?, message_bytes)
//                     .await?;

//             let sig = base64::encode([r, z].concat());
//             let response = serde_json::json!({
//                 "id": parsed["id"],
//                 "result_type": "sign-result",
//                 "data": sig
//             });

//             let _: () = pub_conn.publish("sign-result", response.to_string())?;
//             println!("[CLIENT-SIGN] Published sign result to Redis!");
//         }
//     }
// }
