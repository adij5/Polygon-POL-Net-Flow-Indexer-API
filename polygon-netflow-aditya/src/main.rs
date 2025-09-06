\
use anyhow::Context;
use axum::{routing::get, Json, Router};
use dotenvy::dotenv;
use ethers::providers::{Http, Middleware, Provider};
use ethers::types::{Address, BlockNumber, TxHash, U256};
use serde::Serialize;
use sqlx::{Pool, Postgres};
use std::{collections::HashSet, env, str::FromStr, time::Duration};
use tokio::time::sleep;
use tracing::{error, info};
use tracing_subscriber::{fmt, EnvFilter};

#[derive(Serialize)]
struct NetFlowJson {
    last_block: u64,
    cumulative_in_wei: String,
    cumulative_out_wei: String,
    net_flow_wei: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    fmt().with_env_filter(EnvFilter::from_default_env()).init();

    // Env
    let rpc_url = env::var("POLYGON_RPC").context("POLYGON_RPC not set")?;
    let db_url = env::var("DATABASE_URL").context("DATABASE_URL not set")?;
    let bind_addr = env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());

    // DB
    let pool = sqlx::PgPool::connect(&db_url).await?;
    bootstrap_db(&pool).await?;

    // Provider
    let provider = Provider::<Http>::try_from(rpc_url.clone())
        .with_context(|| format!("Provider init failed for url: {}", rpc_url))?;

    // Binance addresses (from screenshot)
    let binance_addrs = vec![
        "0xF977814e90dA44bFA03b6295A0616a897441aceC",
        "0xe7804c37c13166fF0b37F5aE0BB07A3aEbb6e245",
        "0x505e71695E9bc45943c58adEC1650577BcA68fD9",
        "0x290275e3db66394C52272398959845170E4DCb88",
        "0xD5C08681719445A5Fdce2Bda98b341A49050d821",
        "0x082489A616aB4D46d1947eE3F912e080815b08DA",
    ];
    let binance: HashSet<Address> = binance_addrs
        .into_iter()
        .map(|s| Address::from_str(s).expect("invalid address in list"))
        .collect();

    // Initialize state: no backfill — start from the current latest block
    let latest = provider.get_block_number().await?.as_u64();
    init_state_if_needed(&pool, latest).await?;

    // HTTP server for quick queries
    let pool_clone = pool.clone();
    let app = Router::new().route(
        "/net-flow",
        get(move || get_net_flow(pool_clone.clone())),
    );
    let server = axum::Server::bind(&bind_addr.parse()?)
        .serve(app.into_make_service());

    info!("HTTP listening on {}", bind_addr);

    // Run indexer and server concurrently
    tokio::select! {
        r = run_indexer(provider, pool.clone(), binance) => r?,
        r = server => r.context("server error")?,
    }

    Ok(())
}

async fn bootstrap_db(pool: &Pool<Postgres>) -> anyhow::Result<()> {
    // Create tables if not exist (same as schema.sql)
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS raw_transfers (
            id BIGSERIAL PRIMARY KEY,
            block_number BIGINT NOT NULL,
            tx_hash TEXT NOT NULL,
            "from" TEXT NOT NULL,
            "to" TEXT NOT NULL,
            value_wei TEXT NOT NULL,
            ts TIMESTAMPTZ NOT NULL DEFAULT now()
        );"#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS net_flows_snapshots (
            id BIGSERIAL PRIMARY KEY,
            block_number BIGINT NOT NULL,
            cumulative_in_wei TEXT NOT NULL,
            cumulative_out_wei TEXT NOT NULL,
            net_flow_wei TEXT NOT NULL,
            ts TIMESTAMPTZ NOT NULL DEFAULT now()
        );"#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS indexer_state (
            id SMALLINT PRIMARY KEY DEFAULT 1,
            last_block BIGINT NOT NULL,
            cumulative_in_wei TEXT NOT NULL,
            cumulative_out_wei TEXT NOT NULL
        );"#,
    )
    .execute(pool)
    .await?;

    Ok(())
}

async fn init_state_if_needed(pool: &Pool<Postgres>, latest_block: u64) -> anyhow::Result<()> {
    let exists: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM indexer_state WHERE id = 1")
        .fetch_optional(pool)
        .await?;

    if exists.is_none() {
        sqlx::query(
            "INSERT INTO indexer_state (id, last_block, cumulative_in_wei, cumulative_out_wei) VALUES (1, $1, '0', '0')",
        )
        .bind(latest_block as i64)
        .execute(pool)
        .await?;
    }
    Ok(())
}

async fn run_indexer(
    provider: Provider<Http>,
    pool: Pool<Postgres>,
    binance: HashSet<Address>,
) -> anyhow::Result<()> {
    loop {
        // Current chain head
        let latest = provider.get_block_number().await?.as_u64();

        // Get state
        let (mut last_block, mut cum_in, mut cum_out): (i64, String, String) = sqlx::query_as(
            "SELECT last_block, cumulative_in_wei, cumulative_out_wei FROM indexer_state WHERE id = 1",
        )
        .fetch_one(&pool)
        .await?;

        // Process new blocks only (no backfill)
        if latest > last_block as u64 {
            for n in (last_block as u64 + 1)..=latest {
                if let Some(block) = provider
                    .get_block_with_txs(n)
                    .await
                    .with_context(|| format!("fetch block {}", n))?
                {
                    let mut in_sum = U256::zero();
                    let mut out_sum = U256::zero();

                    for tx in block.transactions {
                        // Native POL value
                        let val = tx.value;
                        if val.is_zero() { continue; }

                        let from = tx.from.unwrap_or_default();
                        let to_opt = tx.to; // Option<Address>

                        if let Some(to) = to_opt {
                            if binance.contains(&to) {
                                in_sum = in_sum.saturating_add(val);
                                // store raw
                                insert_raw(&pool, n, tx.hash, from, to, val).await?;
                            }
                        }
                        if binance.contains(&from) {
                            out_sum = out_sum.saturating_add(val);
                            if let Some(to) = to_opt {
                                insert_raw(&pool, n, tx.hash, from, to, val).await?;
                            }
                        }
                    }

                    // Update running totals
                    let cum_in_u = U256::from_dec_str(&cum_in).unwrap_or_default()
                        .saturating_add(in_sum);
                    let cum_out_u = U256::from_dec_str(&cum_out).unwrap_or_default()
                        .saturating_add(out_sum);
                    let net_u = cum_in_u.saturating_sub(cum_out_u);

                    cum_in = cum_in_u.to_string();
                    cum_out = cum_out_u.to_string();
                    last_block = n as i64;

                    sqlx::query("UPDATE indexer_state SET last_block=$1, cumulative_in_wei=$2, cumulative_out_wei=$3 WHERE id=1")
                        .bind(last_block)
                        .bind(&cum_in)
                        .bind(&cum_out)
                        .execute(&pool)
                        .await?;

                    sqlx::query("INSERT INTO net_flows_snapshots (block_number, cumulative_in_wei, cumulative_out_wei, net_flow_wei) VALUES ($1,$2,$3,$4)")
                        .bind(n as i64)
                        .bind(&cum_in)
                        .bind(&cum_out)
                        .bind(net_u.to_string())
                        .execute(&pool)
                        .await?;

                    info!(block = n, in_wei = %in_sum, out_wei = %out_sum, cum_in = %cum_in, cum_out = %cum_out, "updated totals");
                }
            }
        }

        sleep(Duration::from_secs(6)).await; // ~ block time polling
    }
}

async fn insert_raw(
    pool: &Pool<Postgres>,
    block_number: u64,
    tx_hash: TxHash,
    from: Address,
    to: Address,
    value: U256,
) -> anyhow::Result<()> {
    sqlx::query("INSERT INTO raw_transfers (block_number, tx_hash, \"from\", \"to\", value_wei) VALUES ($1,$2,$3,$4,$5)")
        .bind(block_number as i64)
        .bind(format!("0x{:x}", tx_hash))
        .bind(format!("0x{:x}", from))
        .bind(format!("0x{:x}", to))
        .bind(value.to_string())
        .execute(pool)
        .await?;
    Ok(())
}

async fn get_net_flow(pool: Pool<Postgres>) -> Result<Json<NetFlowJson>, String> {
    let row: (i64, String, String) = sqlx::query_as(
        "SELECT last_block, cumulative_in_wei, cumulative_out_wei FROM indexer_state WHERE id=1",
    )
    .fetch_one(&pool)
    .await
    .map_err(|e| e.to_string())?;

    let (last_block, cin, cout) = row;
    let cin_u = U256::from_dec_str(&cin).unwrap_or_default();
    let cout_u = U256::from_dec_str(&cout).unwrap_or_default();
    let net = cin_u.saturating_sub(cout_u);

    Ok(Json(NetFlowJson {
        last_block: last_block as u64,
        cumulative_in_wei: cin_u.to_string(),
        cumulative_out_wei: cout_u.to_string(),
        net_flow_wei: net.to_string(),
    }))
}
