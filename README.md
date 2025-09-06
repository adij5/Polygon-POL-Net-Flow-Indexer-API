# Polygon POL Net-Flow Indexer & API

**By**: Aditya Jaiswal

**Project Overview**

This project is a blockchain data indexer and API service.

It monitors the Polygon blockchain in real-time and tracks the net flow
of MATIC tokens (POL) into and out of specific Binance exchange
addresses.

The data is stored in a PostgreSQL database and made available via a
REST API endpoint (/net-flow).

**Key Features**

1\. Blockchain Indexer

-   Connects to Polygon RPC.
-   Reads new blocks continuously.
-   Filters transactions involving Binance hot wallets.
-   Tracks inflows (to Binance) and outflows (from Binance).

2\. Database Integration

-   Stores raw transfers (transaction-level details).
-   Keeps cumulative totals of inflow and outflow.
-   Maintains snapshots for historical queries.
-   Maintains state so it can resume after restart.

3\. REST API

-   Exposes /net-flow endpoint.
-   Returns the latest cumulative inflows, outflows, and net flow in
    JSON.

**Tech Stack**

-   Language: Rust (fast, memory-safe, async capable).
-   Blockchain SDK: ethers-rs a (for Polygon RPC).
-   Framework: Axum a (HTTP server).
-   Database: PostgreSQL.
-   ORM: SQLX 7.
-   Runtime: Tokio.
-   Logging: Tracing.

**Note (**To run)

-   Set **POLYGON_RPC** and **DATABASE_URL** in a **. env** (I have
    included **.env. example** ), run the **schema.sql** against your
    Postgres DB, then **cargo run \--release**.
    **Example of Transactions**


**Reference** (since Rust and Blockchain is new for me)

-   Google
-   Youtube
-   chatgpt
