use anyhow::Result;
use dkg_tcp::{config, keygen, sign};
use futures::StreamExt;
use givre::generic_ec::curves::Ed25519;
use givre::key_share::DirtyKeyShare;
use givre::keygen::key_share::Valid;
use redis::aio::{MultiplexedConnection, PubSub};
use redis::{AsyncCommands, Client};
use serde_json;
use solana_sdk::signature::Signature;
// use sqlx::{Pool, Postgres, Row};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::{net::TcpListener, task};

type ShareStore = Arc<RwLock<HashMap<(u64, String), Valid<DirtyKeyShare<Ed25519>>>>>;

pub async fn run_server() -> Result<()> {
    let id: u64 = 0;
    let config = config::Config::load();

    println!("[SERVER] Using Redis: {}", config.redis_url);

    // ✅ Shared Redis + Postgres (async)
    let redis_client_dkg = Arc::new(Client::open(config.redis_url.clone())?);
    let redis_client_sign = Arc::new(Client::open(config.redis_url.clone())?);
    // let db_pool: Arc<Pool<Postgres>> = Arc::new(config::get_db_pool().await?);
    println!("[SERVER] Connected to PostgreSQL");

    // shared in-memory store
    let share_store: ShareStore = Arc::new(RwLock::new(HashMap::new()));

    // ✅ Run DKG and Sign servers concurrently
    let dkg_task = {
        let redis = redis_client_dkg.clone();
        // let db = db_pool.clone();
        let store = share_store.clone();
        task::spawn(async move {
            if let Err(e) = run_dkg_server(redis, store, id).await {
                eprintln!("[DKG] Error: {:?}", e);
            }
        })
    };

    let sign_task = {
        let redis = redis_client_sign.clone();
        // let db = db_pool.clone();
        let store = share_store.clone();
        task::spawn(async move {
            if let Err(e) = run_sign_server(redis, store, id).await {
                eprintln!("[SIGN] Error: {:?}", e);
            }
        })
    };

    let _ = tokio::join!(dkg_task, sign_task);
    Ok(())
}

async fn run_dkg_server(redis_client: Arc<Client>, share_store: ShareStore, id: u64) -> Result<()> {
    // ✅ Latest Redis crate: use get_async_pubsub()
    let mut pubsub = redis_client.get_async_pubsub().await?;
    pubsub.subscribe("dkg-start").await?;
    println!("[DKG] Listening on Redis channel `dkg-start`");

    // ✅ Separate multiplexed connection for publishing normal commands
    let mut pub_conn: MultiplexedConnection =
        redis_client.get_multiplexed_async_connection().await?;

    let listener = TcpListener::bind("0.0.0.0:7001").await?;
    println!("[DKG] TCP listener active on port 7001");

    // ✅ Stream messages from PubSub
    while let Some(msg) = pubsub.on_message().next().await {
        let payload: String = msg.get_payload()?;
        println!("[DKG] Redis msg: {}", payload);

        let parsed: serde_json::Value = serde_json::from_str(&payload)?;
        if parsed["action"] == "startdkg" {
            let session = parsed["session"].as_str().unwrap_or("session-001");
            println!("[DKG] Starting keygen session {}", session);

            let (socket, addr) = listener.accept().await?;
            println!("[DKG] Connected to peer at {:?}", addr);

            let shares: Valid<DirtyKeyShare<Ed25519>> =
                keygen::generate_private_share(socket, id, session.as_bytes()).await?;
            let pubkey: String =
                bs58::encode(shares.shared_public_key().to_bytes(true)).into_string();

            // store in-memory for now
            {
                let mut store = share_store.write().await;
                store.insert((id, session.to_string()), shares.clone());
            }
            println!("[DKG] ✅ Stored share in memory for session {}", session);

            // ✅ Store share in Postgres
            // let encrypted_share = serde_json::to_string(&shares)?;
            // println!("[DKG] Encrypted share length = {}", encrypted_share.len());

            // sqlx::query(
            //     "INSERT INTO share_schema.shares (nodeId, sessionId, solanaAddress, encryptedShare)
            //      VALUES ($1, $2, $3, $4)",
            // )
            // .bind(id as i64)
            // .bind(session)
            // .bind(&pubkey)
            // .bind(&encrypted_share)
            // .execute(&*db_pool)
            // .await?;

            // println!("[DKG] Stored encrypted share for session {}", session);

            // ✅ Publish result asynchronously
            let response = serde_json::json!({
                "id": parsed["id"],
                "result_type": "dkg-result",
                "data": pubkey,
                "server_id": id,
            });
            pub_conn
                .publish::<_, _, ()>("dkg-result", response.to_string())
                .await?;
            println!("[DKG] Result published!");
        }
    }
    Ok(())
}

pub async fn run_sign_server(
    redis_client: Arc<redis::Client>,
    share_store: ShareStore,
    id: u64,
) -> Result<()> {
    // ✅ Redis pub/sub setup
    let mut pubsub: PubSub = redis_client.get_async_pubsub().await?;
    pubsub.subscribe("sign-start").await?;
    println!("[SIGN] Listening on Redis channel `sign-start`");

    // ✅ TCP listener setup
    let listener = TcpListener::bind("0.0.0.0:7002").await?;
    println!("[SIGN] TCP listener active on port 7002");

    // ✅ Connection for publishing
    let mut pub_conn: MultiplexedConnection =
        redis_client.get_multiplexed_async_connection().await?;
    let mut on_msg = pubsub.on_message();

    while let Some(msg) = on_msg.next().await {
        // --- Handle payload safely ---
        let payload: String = match msg.get_payload() {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[SIGN] Failed to parse payload: {:?}", e);
                continue;
            }
        };

        println!("[SIGN] Redis msg: {}", payload);

        // --- Parse JSON safely ---
        let parsed: serde_json::Value = match serde_json::from_str(&payload) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[SIGN] Invalid JSON payload: {:?}", e);
                continue;
            }
        };

        // --- Ignore unrelated actions ---
        if parsed["action"] != "sign" {
            println!("[SIGN] Ignored unrelated message.");
            continue;
        }

        let session = parsed["session"].as_str().unwrap_or("session-001");
        println!("[SIGN] Starting signing for session {}", session);

        // --- Accept TCP connection ---
        let (socket, addr) = match listener.accept().await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[SIGN] TCP accept error: {:?}", e);
                continue;
            }
        };
        println!("[SIGN] Connected to peer at {:?}", addr);

        // --- Fetch from in-memory store ---
        let maybe_share = {
            let store = share_store.read().await;
            store.get(&(id, session.to_string())).cloned()
        };

        let valid_shares = match maybe_share {
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

        // --- Decode message ---
        let message_base64 = parsed["message"].as_str().unwrap_or_default();
        let message_bytes = match base64::decode(message_base64) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("[SIGN] Failed to decode message: {:?}", e);
                continue;
            }
        };

        // --- Run signing process ---
        match sign::run_signing_phase(id, valid_shares, socket, message_bytes).await {
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
                println!("[SIGN] ✅ Signature published!");
            }
            Err(e) => {
                eprintln!("[SIGN] Signing phase failed: {:?}", e);
                let fail_ack = serde_json::json!({
                    "id": parsed["id"],
                    "result_type": "sign-error",
                    "error": format!("Signing failed: {}", e),
                    "server_id": id,
                });
                let _ = pub_conn
                    .publish::<_, _, ()>("sign-result", fail_ack.to_string())
                    .await;
                continue;
            }
        }
    }

    Ok(())
}
