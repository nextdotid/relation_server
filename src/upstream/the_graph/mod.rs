mod tests;

use crate::config::C;
use crate::graph::edge::hold::Hold;
use crate::graph::vertex::{contract::Chain, contract::ContractCategory, Contract};
use crate::upstream::{DataSource, DataFetcher, Platform, Fetcher, Target, TargetProcessedList};
use crate::util::naive_now;
use crate::{
    error::Error,
    graph::{create_identity_to_contract_record, new_db_connection, vertex::Identity},
};
use async_trait::async_trait;
use gql_client::Client;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize, Debug)]
pub struct ResolvedAddress {
    id: String,
}

#[derive(Deserialize, Debug)]
pub struct EthQueryResponseDomains {
    id: String,
    name: String,
    labelName: String,
    labelhash: String,
    resolvedAddress: Option<ResolvedAddress>,
}

#[derive(Deserialize, Debug)]
pub struct Domain {
    name: String,
}

#[derive(Deserialize, Debug)]
pub struct EthQueryResponseTransfers {
    domain: Domain,
    transactionID: String,
}

#[derive(Deserialize, Debug)]
pub struct EthQueryResponse {
    domains: Vec<EthQueryResponseDomains>,
    transfers: Option<Vec<EthQueryResponseTransfers>>,
}

#[derive(Serialize)]
pub struct EthQueryVars<'a> {
    addr: &'a str,
}

#[derive(Serialize)]
pub struct ENSQueryVars {
    ens: String,
}

pub struct TheGraph {}

#[async_trait]
impl Fetcher for TheGraph {
    async fn fetch(target: &Target) -> Result<TargetProcessedList, Error> {
        if !Self::can_fetch(target) {
            return Ok(vec![]);
        }

        match target {
            Target::Identity(_, identity) => fetch_ens_by_eth_wallet(identity).await,
            Target::NFT(_, _, _, id) => fetch_eth_wallet_by_ens(id).await,
        }
    }

    fn can_fetch(target: &Target) -> bool {
        target.in_platform_supported(vec![Platform::Ethereum])
            || target.in_nft_supported(vec![ContractCategory::ENS], vec![Chain::Ethereum])
    }
}

/// Use ethereum address to fetch NFTs (especially ENS).
async fn fetch_ens_by_eth_wallet(identity: &str) -> Result<TargetProcessedList, Error> {
    let query = r#"
        query EnsByOwnerAddress($addr: String!){
            domains(where: { owner: $addr}) {
                id
                name
                labelName
                labelhash
                resolvedAddress {
                  id
                }
              }
        }
    "#;

    let client = Client::new(C.upstream.the_graph_service.url.clone());
    let vars = EthQueryVars {
        addr: &identity.to_lowercase(),
    };

    let resp = client
        .query_with_vars::<EthQueryResponse, EthQueryVars>(query, vars)
        .await;

    if resp.is_err() {
        warn!(
            "The Graph fetch | Failed to fetch addrs: {}, err: {:?}",
            identity,
            resp.err()
        );
        return Ok(vec![]);
    }

    let res = resp.unwrap().unwrap();
    if res.domains.is_empty() {
        info!(
            "The Graph fetch | address: {} cannot find any result",
            identity
        );
        return Ok(vec![]);
    }
    let db = new_db_connection().await?;
    let mut next_targets: TargetProcessedList = Vec::new();

    for domain in res.domains.iter() {
        if domain.resolvedAddress.is_none()
            || domain.resolvedAddress.as_ref().unwrap().id != identity
        {
            continue;
        }
        let from: Identity = Identity {
            uuid: Some(Uuid::new_v4()),
            platform: Platform::Ethereum,
            identity: identity.to_lowercase(),
            created_at: None,
            display_name: identity.to_lowercase(),
            added_at: naive_now(),
            avatar_url: None,
            profile_url: None,
            updated_at: naive_now(),
        };
        let to: Contract = Contract {
            uuid: Uuid::new_v4(),
            category: ContractCategory::ENS,
            address: ContractCategory::ENS.default_contract_address().unwrap(),
            chain: Chain::Ethereum,
            symbol: None,
            updated_at: naive_now(),
        };
        let ens = domain.to_owned().name.clone();
        let ownership: Hold = Hold {
            uuid: Uuid::new_v4(),
            transaction: None,
            id: ens.clone(),
            source: DataSource::TheGraph,
            created_at: None,
            updated_at: naive_now(),
            fetcher: DataFetcher::RelationService,
        };
        create_identity_to_contract_record(&db, &from, &to, &ownership).await?;
        next_targets.push(Target::NFT(
            Chain::Ethereum,
            ContractCategory::ENS,
            ContractCategory::ENS.default_contract_address().unwrap(),
            ens.clone(),
        ));
    }
    Ok(next_targets)
}

async fn fetch_eth_wallet_by_ens(ens_str: &str) -> Result<TargetProcessedList, Error> {
    let query = r#"
        query QueryAddressByENS($ens: String!){
            domains(where: { name: $ens}) {
                id
                name
                labelName
                labelhash
                resolvedAddress {
                  id
                }
              }
            transfers (where:{ domain_: { name : $ens}}) {
                domain {
                  name
                }
                transactionID
              }
        }
    "#;
    let client = Client::new(C.upstream.the_graph_service.url.clone());
    let vars = ENSQueryVars {
        ens: ens_str.to_string(),
    };
    let response = client
        .query_with_vars::<EthQueryResponse, ENSQueryVars>(query, vars)
        .await;

    if response.is_err() {
        warn!(
            "The Graph fetch | Failed to fetch addrs using ENS: {}, error: {:?}",
            ens_str,
            response.err()
        );
        return Ok(vec![]);
    }
    let res = response.unwrap().unwrap();


    if res.domains.is_empty() {
        info!(
            "The Graph fetch | ens: {} cannot find any result",
            ens_str.to_string()
        );
        return Ok(vec![]);
    }

    // ens => addr use the first result
    let address = &res
        .domains
        .first()
        .unwrap()
        .resolvedAddress
        .as_ref()
        .unwrap()
        .id;

    let mut tx_id = None;
    if res.transfers.is_some() {
        let transfers = res.transfers.unwrap();
        let transfer = transfers.first().unwrap();
        tx_id = Some(transfer.transactionID.clone());
    }

    let from = Identity {
        uuid: Some(Uuid::new_v4()),
        platform: Platform::Ethereum,
        identity: address.to_string(),
        display_name: address.to_string(),
        profile_url: None,
        avatar_url: None,
        created_at: None,
        added_at: naive_now(),
        updated_at: naive_now(),
    };
    let to = Contract {
        uuid: Uuid::new_v4(),
        updated_at: naive_now(),
        category: ContractCategory::ENS,
        address: ContractCategory::ENS.default_contract_address().unwrap(),
        chain: Chain::Ethereum,
        symbol: None,
    };
    let hold = Hold {
        uuid: Uuid::new_v4(),
        transaction: tx_id,
        id: ens_str.to_string(),
        source: DataSource::TheGraph,
        created_at: None,
        updated_at: naive_now(),
        fetcher: DataFetcher::RelationService,
    };
    let db = new_db_connection().await?;
    create_identity_to_contract_record(&db, &from, &to, &hold).await?;

    Ok(vec![Target::Identity(
        Platform::Ethereum,
        address.to_string(),
    )])
}
