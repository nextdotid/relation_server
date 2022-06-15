mod tests;

use crate::{error::Error, graph::{new_db_connection, vertex::Identity, edge::Proof}};
use crate::graph::{Vertex, Edge};
use serde::Deserialize;
use crate::util::{naive_now, timestamp_to_naive, make_client, parse_body};
use async_trait::async_trait;
use crate::upstream::{Fetcher, Platform, DataSource, Connection};
use uuid::Uuid;
use std::str::FromStr;
use chrono::{DateTime, NaiveDateTime};
use futures::{future::join_all};


#[derive(Deserialize, Debug)]
pub struct Rss3Response {
    pub version: String,
    pub date_updated: String,
    pub identifier: String,
    pub total: i64,
    pub list: Vec<Item>,
}

#[derive(Deserialize, Debug)]
pub struct Item {
    pub date_created: String,
    pub date_updated: String,
    pub related_urls: Vec<String>,
    #[serde(default)] 
    pub title: String,
    pub source: String,
    pub metadata: MetaData,
}

#[derive(Deserialize, Debug)]
pub struct MetaData {
    #[serde(default)] 
    pub collection_address: String,
    #[serde(default)] 
    pub collection_name: String,
    #[serde(default)] 
    pub contract_type: String,
    pub from: String,
    #[serde(default)] 
    pub log_index: String,
    pub network: String,
    pub proof: String,
    pub to: String,
    #[serde(default)] 
    pub token_id: String,
    pub token_standard: String,
    pub token_symbol: String,
}

#[derive(Deserialize, Debug)]
pub struct ErrorResponse {
    pub message: String,
}

pub struct Rss3 {
    pub network: String,
    pub account: String,
    pub tags: String,
}

async fn save_item(p: Item) -> Option<Connection> {
    
    let create_date_time = DateTime::parse_from_rfc3339(&p.date_created).unwrap();
    let create_naive_date_time = NaiveDateTime::from_timestamp(create_date_time.timestamp(), 0);
    let update_date_time = DateTime::parse_from_rfc3339(&p.date_updated).unwrap();
    let update_naive_date_time = NaiveDateTime::from_timestamp(update_date_time.timestamp(), 0);

    let db = new_db_connection().await.ok()?;

    let from: Identity = Identity {
        uuid: Some(Uuid::new_v4()),
        platform: Platform::Ethereum,
        identity: p.metadata.to.clone(),
        created_at: None,
        display_name: p.metadata.to.clone(),
        added_at: naive_now(),
        avatar_url: None,
        profile_url: None,
        updated_at: naive_now(),
    };

    let from_record = from.create_or_update(&db).await.ok()?;

    let to: Identity = Identity {
        uuid: Some(Uuid::new_v4()),
        platform: Platform::Ethereum,
        identity: p.metadata.collection_address.clone(),
        created_at: Some(create_naive_date_time),
        display_name: p.metadata.collection_address.clone(),
        added_at: naive_now(),
        avatar_url: None,
        profile_url: None,
        updated_at: naive_now(),
    };
    let to_record = to.create_or_update(&db).await.ok()?;


    let pf: Proof = Proof {
        uuid: Uuid::new_v4(),
        source: DataSource::Rss3,
        record_id: Some(p.metadata.proof.clone()),
        created_at: Some(create_naive_date_time), 
        last_fetched_at: update_naive_date_time,
    };

    let proof_record = pf.connect(&db, &from_record, &to_record).await.ok()?;

    let cnn: Connection = Connection {
        from: from_record,
        to: to_record,
        proof: proof_record,
    };
    
    return Some(cnn);
}

#[async_trait]
impl Fetcher for Rss3 {
    async fn fetch(&self, _url: Option<String>) -> Result<Vec<Connection>, Error> { 
        let client = make_client();
        let uri: http::Uri = match format!("https://pregod.rss3.dev/v0.4.0/account:{}@{}/notes?tags={}", self.account, self.network, self.tags).parse() {
            Ok(n) => n,
            Err(err) => return Err(Error::ParamError(
                format!("Uri format Error: {}", err.to_string()))),
        };
  
        let mut resp = client.get(uri).await?;

        if !resp.status().is_success() {
            let body: ErrorResponse = parse_body(&mut resp).await?;
            return Err(Error::General(
                format!("Rss3 Result Get Error: {}", body.message),
                resp.status(),
            ));
        }

        let body: Rss3Response = parse_body(&mut resp).await?;  
       
        if body.total == 0 {
            return Err(Error::General(
                format!("rss3 Result Get Error"),
                resp.status(),
            )); 
        }

        // parse 
        let futures :Vec<_> = body.list.into_iter().filter(|p| p.metadata.to == self.account.to_lowercase()).map(|p| save_item(p)).collect();
        let results = join_all(futures).await;
        let parse_body: Vec<Connection> = results.into_iter().filter_map(|i|i).collect();

        Ok(parse_body)
    }
}