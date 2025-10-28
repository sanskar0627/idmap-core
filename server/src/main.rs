mod server;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    server::run_server().await
}

// use dkg_tcp::{config, keygen, sign};

// use anyhow::Result;
// use bincode;
// use hex;
// use redis::Commands;
// use sqlx::Row;
// use sqlx::{Pool, Postgres};
// use std::sync::Arc;
// use tokio::net::TcpListener;

// use solana_message::Message;
// use solana_pubkey::Pubkey;
// use solana_rpc_client::rpc_client::RpcClient;
// use solana_sdk::signature::Signature;
// use solana_transaction::Transaction;

// // returning: Result<(), RedisError>
// #[tokio::main]
// async fn main() -> Result<()> {
//     let id: u64 = 0;

//     // configurations using config.rs and env file
//     let config = config::Config::load(); // use redis url from local env directly
//     println!("[SERVER] using redis: {}", config.redis_url);

//     let db_pool: Pool<Postgres> = config::get_db_pool().await?;
//     println!("[SERVER] Connected to PostgreSQL");

//     let listener: TcpListener = TcpListener::bind("0.0.0.0:7000").await?;
//     println!("[SERVER] Listening on 0.0.0.0:7000");

//     let (socket, addr) = listener.accept().await?;
//     println!("[SERVER] Client connected from {:?}", addr);

//     // Convert to std
//     let std_stream: std::net::TcpStream = socket.into_std()?;
//     std_stream.set_nonblocking(true)?;

//     // Clone for DKG and signing phases
//     let std_stream_dkg: std::net::TcpStream = std_stream.try_clone()?;
//     let std_stream_sign: std::net::TcpStream = std_stream.try_clone()?;
//     let std_stream_send: std::net::TcpStream = std_stream.try_clone()?;

//     // ================= DKG PHASE ================= [should be called only once per user]
//     let session: &'static [u8; 11] = b"session-001"; // make it dynamic
//     let valid_shares = keygen::generate_private_share(std_stream_dkg, id, session).await?;

//     let pubkey_bytes = valid_shares.shared_public_key().to_bytes(true);
//     let solana_address: String = bs58::encode(pubkey_bytes).into_string();
//     println!("SOLANA ADDRESS: {}", solana_address);
//     println!(
//         "shared public key (hex): {}",
//         hex::encode(valid_shares.shared_public_key().to_bytes(true))
//     );
//     println!("finished DKG for server");

//     // store the encrypted share into the database (for now)
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

//     println!("[SERVER] Stored encrypted DKG share in Postgres");

//     // airdrop some sol after generating keys for testing
//     // let pubkey = keygen::airdrop_funds(&solana_address, 5_000_000)?;
//     // std::thread::sleep(std::time::Duration::from_secs(60));
//     let pubkey: Pubkey = Pubkey::from_str_const(&solana_address);

//     // ================= SIGNING PHASE =================
//     let lamports: u64 = 1_000_000;
//     let to: Pubkey = Pubkey::from_str_const("3fHQhTVCHerqk69GwqXhWbG4zRArCUqi6Bhnu7pTm5mj"); // get this from the user

//     // create the message to sign
//     let message: Message =
//         sign::create_transfer_message(&solana_address, &to.to_string(), lamports)
//             .expect("error generating message to be signed");

//     // send to the other server from here
//     let _ = sign::send_message_to_other_server(id, std_stream_send, message.clone()).await?;

//     let row: sqlx::postgres::PgRow = sqlx::query(
//         "SELECT encryptedShare FROM share_schema.shares WHERE nodeId = $1 AND sessionId = $2",
//     )
//     .bind(id as i64)
//     .bind(session)
//     .fetch_one(&db_pool)
//     .await?;

//     let encrypted_share: Vec<u8> = row.try_get("encryptedShare")?;
//     let valid_shares = bincode::deserialize(&encrypted_share)?;

//     println!("[SERVER] Retrieved and decrypted share from DB");

//     // start signing protocol
//     let (r_slice, z_slice) =
//         sign::run_signing_phase(id, valid_shares, std_stream_sign, message.serialize()).await?;

//     // ================= BROADCAST TO THE SOLANA BLOCKCHAIN =================

//     println!("broadcasting to the solana");
//     let rpc: RpcClient = RpcClient::new("https://api.devnet.solana.com".to_string());

//     // for testing
//     let balance = rpc.get_balance(&pubkey)?;
//     println!("[SERVER] Balance after airdrop: {}", balance);
//     if balance < 1_000_000 {
//         panic!("Not enough funds to send transaction!");
//     }

//     let mut tx: Transaction = Transaction::new_unsigned(message);
//     let sig_bytes: Vec<u8> = [r_slice, z_slice].concat();
//     let sol_sig: Signature = Signature::try_from(sig_bytes.clone())
//         .expect("err creating solana signature from dkg signature");

//     tx.signatures = vec![sol_sig];
//     let tx_sig: Signature = rpc.send_transaction(&tx)?;
//     println!("Broadcasted tx: {}", tx_sig);

//     Ok(())
// }

// async fn run_dkg_server(redis_client: Arc<redis::Client>, id: u64) -> Result<()> {
//     let mut pub_conn = redis_client.get_connection()?;
//     let mut sub_conn = redis_client.get_connection()?;
//     let mut pubsub = sub_conn.as_pubsub();

//     let db_pool: Pool<Postgres> = config::get_db_pool().await?;
//     println!("[SERVER] Connected to PostgreSQL");

//     pubsub.subscribe("dkg-start")?;
//     println!("[DKG] Listening on Redis channel `dkg-start`");

//     let listener = tokio::net::TcpListener::bind("0.0.0.0:7001").await?; // TODO: update the port
//     println!("[DKG] TCP listener active on port 7001");

//     let (socket, addr) = listener.accept().await?;
//     println!("[DKG] Connected to peer at {:?}", addr);

//     // let std_stream = socket.into_std()?;
//     // std_stream.set_nonblocking(true)?;
//     // let stream_clone = std_stream.try_clone()?;

//     loop {
//         let msg = pubsub.get_message()?;
//         let payload: String = msg.get_payload()?;
//         println!("[DKG] Redis msg: {}", payload);

//         // TODO: validate the payload

//         let parsed: serde_json::Value = serde_json::from_str(&payload)?;
//         let session = parsed["session"].clone().to_string();

//         if parsed["action"] == "startdkg" {
//             println!("[DKG] Starting keygen...");

//             let shares =
//                 keygen::generate_private_share(socket, 0, session.as_bytes())
//                     .await?;
//             let pubkey = bs58::encode(shares.shared_public_key().to_bytes(true)).into_string();

//             let pubkey_bytes = shares.shared_public_key().to_bytes(true);
//             let solana_address = bs58::encode(pubkey_bytes).into_string();

//             // store the encrypted share into the database (for now)
//             let encrypted_share: Vec<u8> = bincode::serialize(&shares)?.to_vec(); // TODO: encrypt it
//             sqlx::query(
//                 "INSERT INTO share_schema.shares (nodeId, sessionId, solanaAddress, encryptedShare)
//                     VALUES ($1, $2, $3, $4)",
//             )
//             .bind(id as i64)
//             .bind(session)
//             .bind(&solana_address)
//             .bind(&encrypted_share)
//             .execute(&db_pool)
//             .await?;

//             println!("[SERVER] Stored encrypted DKG share in Postgres");

//             // send the response to the node backend
//             let response = serde_json::json!({
//                 "id": parsed["id"],
//                 "result_type": "dkg-result",
//                 "data": pubkey
//             });

//             let _: () = pub_conn.publish("dkg-result", response.to_string())?;
//             println!("[DKG] Result published!");
//         }
//     }
// }

// async fn run_sign_server(redis_client: Arc<redis::Client>, id: u64) -> Result<()> {
//     let mut pub_conn = redis_client.get_connection()?;
//     let mut sub_conn = redis_client.get_connection()?;
//     let mut pubsub = sub_conn.as_pubsub();

//     pubsub.subscribe("sign-start")?;
//     println!("[SIGN] Listening on Redis channel `sign-start`");

//     let listener = tokio::net::TcpListener::bind("0.0.0.0:7002").await?; // TODO: update the port
//     println!("[SIGN] TCP listener active on port 7002");

//     let (socket, addr) = listener.accept().await?;
//     println!("[SIGN] Connected to peer at {:?}", addr);

//     let db_pool: Pool<Postgres> = config::get_db_pool().await?;
//     println!("[SERVER] Connected to PostgreSQL");

//     let std_stream = socket.into_std()?;
//     std_stream.set_nonblocking(true)?;
//     let stream_clone = std_stream.try_clone()?;

//     loop {
//         let msg = pubsub.get_message()?;
//         let payload: String = msg.get_payload()?;
//         println!("[SIGN] Redis msg: {}", payload);

//         let parsed: serde_json::Value = serde_json::from_str(&payload)?;
//         // TODO: validate the parsed data

//         let session = parsed["session"].to_string();

//         if parsed["action"] == "sign" {
//             println!("[SIGN] Starting signing...");

//             // get the share from the database
//             let row: sqlx::postgres::PgRow = sqlx::query(
//                 "SELECT encryptedShare FROM share_schema.shares WHERE nodeId = $1 AND sessionId = $2",
//             )
//             .bind(id as i64)
//             .bind(session)
//             .fetch_one(&db_pool)
//             .await?;

//             let encrypted_share: Vec<u8> = row.try_get("encryptedShare")?;
//             let valid_shares = bincode::deserialize(&encrypted_share)?;

//             println!("[SERVER] Retrieved and decrypted share from DB");

//             let message_bytes = base64::decode(parsed["message"].as_str().unwrap())?;
//             let (r, z) =
//                 sign::run_signing_phase(0, valid_shares, stream_clone.try_clone()?, message_bytes)
//                     .await?;

//             let sig = base64::encode([r, z].concat());
//             let response = serde_json::json!({
//                 "id": parsed["id"],
//                 "result_type": "sign-result",
//                 "data": sig
//             });

//             let _: () = pub_conn.publish("sign-result", response.to_string())?;
//             println!("[SIGN] Result published!");
//         }
//     }
// }
