use anyhow::Result;
use dkg_tcp::{config, keygen, sign};
use futures::StreamExt;
use redis::aio::{MultiplexedConnection, PubSub};
use redis::{AsyncCommands, Client};
use solana_sdk::signature::Signature;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;
use tokio::{net::TcpStream, task};

use givre::generic_ec::curves::Ed25519;
use givre::key_share::DirtyKeyShare;
use givre::keygen::key_share::Valid;

type ShareStore = Arc<RwLock<HashMap<(u64, String), Valid<DirtyKeyShare<Ed25519>>>>>;

pub async fn run_client() -> Result<()> {
    let id: u64 = 1;
    let config = config::Config::load();

    println!("[CLIENT] Using Redis: {}", config.redis_url);
    let redis_client_dkg = Arc::new(Client::open(config.redis_url.clone())?);
    let redis_client_sign = Arc::new(Client::open(config.redis_url.clone())?);
    // let db_pool: Arc<Pool<Postgres>> = Arc::new(config::get_db_pool().await?);
    // println!("[CLIENT] Connected to PostgreSQL");

    // shared in-memory store
    let share_store: ShareStore = Arc::new(RwLock::new(HashMap::new()));

    // ✅ Run both client handlers concurrently
    let dkg_client = {
        let redis = redis_client_dkg.clone();
        // let db = db_pool.clone();
        let store = share_store.clone();
        task::spawn(async move {
            if let Err(e) = run_dkg_client(redis, store, id).await {
                eprintln!("[CLIENT-DKG] Error: {:?}", e);
            }
        })
    };

    let sign_client = {
        let redis = redis_client_sign.clone();
        // let db = db_pool.clone();
        let store = share_store.clone();
        task::spawn(async move {
            if let Err(e) = run_sign_client(redis, store, id).await {
                eprintln!("[CLIENT-SIGN] Error: {:?}", e);
            }
        })
    };

    println!("[CLIENT] DKG + SIGN running concurrently...");
    let _ = tokio::join!(dkg_client, sign_client);
    Ok(())
}

async fn run_dkg_client(redis_client: Arc<Client>, share_store: ShareStore, id: u64) -> Result<()> {
    // ✅ Updated for redis 0.32.x
    let mut pubsub: PubSub = redis_client.get_async_pubsub().await?;
    pubsub.subscribe("dkg-start").await?;
    println!("[CLIENT-DKG] Subscribed to `dkg-start`");

    let mut pub_conn: MultiplexedConnection =
        redis_client.get_multiplexed_async_connection().await?;
    let mut on_msg = pubsub.on_message();

    while let Some(msg) = on_msg.next().await {
        let payload: String = msg.get_payload()?;
        println!("[CLIENT-DKG] Received: {}", payload);

        let parsed: serde_json::Value = serde_json::from_str(&payload)?;
        if parsed["action"] == "startdkg" {
            let session = parsed["session"].as_str().unwrap_or("session-001");
            println!("[CLIENT-DKG] Starting DKG session {}", session);

            let socket: TcpStream = TcpStream::connect("127.0.0.1:7001").await?;
            let shares = keygen::generate_private_share(socket, id, session.as_bytes()).await?;
            let pubkey = bs58::encode(shares.shared_public_key().to_bytes(true)).into_string();

            // store in-memory for now
            {
                let mut store = share_store.write().await;
                store.insert((id, session.to_string()), shares.clone());
            }
            println!("[DKG] ✅ Stored share in memory for session {}", session);

            let response = serde_json::json!({
                "id": parsed["id"],
                "result_type": "dkg-result",
                "data": pubkey,
                "server_id": id,
            });
            pub_conn
                .publish::<_, _, ()>("dkg-result", response.to_string())
                .await?;
            println!("[CLIENT-DKG] DKG result published!");
        }
    }
    Ok(())
}

async fn run_sign_client(
    redis_client: Arc<Client>,
    share_store: ShareStore,
    id: u64,
) -> Result<()> {
    // ✅ Subscribe to Redis
    let mut pubsub: PubSub = redis_client.get_async_pubsub().await?;
    pubsub.subscribe("sign-start").await?;
    println!("[CLIENT-SIGN] Subscribed to `sign-start`");

    let mut pub_conn: MultiplexedConnection =
        redis_client.get_multiplexed_async_connection().await?;
    let mut on_msg = pubsub.on_message();

    while let Some(msg) = on_msg.next().await {
        let payload: String = match msg.get_payload() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[CLIENT-SIGN] Failed to parse payload: {:?}", e);
                continue;
            }
        };

        println!("[CLIENT-SIGN] Received: {}", payload);

        // --- Parse incoming JSON ---
        let parsed: serde_json::Value = match serde_json::from_str(&payload) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[CLIENT-SIGN] Invalid JSON payload: {:?}", e);
                continue;
            }
        };

        if parsed["action"] != "sign" {
            println!("[CLIENT-SIGN] Ignored unrelated message.");
            continue;
        }

        let session = parsed["session"].as_str().unwrap_or("session-001");
        println!("[CLIENT-SIGN] Signing for session {}", session);

        // --- Fetch from in-memory store ---
        let maybe_share = {
            let store = share_store.read().await;
            store.get(&(id, session.to_string())).cloned()
        };

        let valid_share = match maybe_share {
            Some(s) => s,
            None => {
                eprintln!(
                    "[SIGN] ❌ No in-memory share found for node {} and session {}",
                    id, session
                );
                let error_ack = serde_json::json!({
                    "id": parsed["id"],
                    "result_type": "sign-error",
                    "error": format!("No in-memory share found for node {} session {}", id, session),
                    "server_id": id,
                });
                let _ = pub_conn
                    .publish::<_, _, ()>("sign-result", error_ack.to_string())
                    .await;
                continue;
            }
        };

        // --- Decode message to sign ---
        let message_base64 = parsed["message"].as_str().unwrap_or_default();
        let message_bytes = match base64::decode(message_base64) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("[CLIENT-SIGN] Failed to decode message: {:?}", e);
                continue;
            }
        };

        // --- Signing Phase ---
        match TcpStream::connect("127.0.0.1:7002").await {
            Ok(socket) => {
                match sign::run_signing_phase(id, valid_share, socket, message_bytes).await {
                    Ok((r, z)) => {
                        let sig = [r, z].concat();
                        // convert this sig into sol_sig and return that to the node backend
                        let sol_sig: Signature = Signature::try_from(sig.clone())
                            .expect("err creating solana signature from dkg signature");
                        let response = serde_json::json!({
                            "id": parsed["id"],
                            "result_type": "sign-result",
                            "data": sol_sig.to_string(),
                            "server_id": id,
                        });
                        pub_conn
                            .publish::<_, _, ()>("sign-result", response.to_string())
                            .await?;
                        println!("[CLIENT-SIGN] ✅ Signature published!");
                    }
                    Err(e) => {
                        eprintln!("[CLIENT-SIGN] Signing phase failed: {:?}", e);
                        let fail_ack = serde_json::json!({
                            "id": parsed["id"],
                            "result_type": "sign-error",
                            "error": format!("Signing failed: {}", e),
                            "server_id": id,
                        });
                        let _ = pub_conn
                            .publish::<_, _, ()>("sign-result", fail_ack.to_string())
                            .await;
                    }
                }
            }
            Err(e) => {
                eprintln!("[CLIENT-SIGN] TCP connection error: {:?}", e);
                continue;
            }
        };
    }

    Ok(())
}
