use crate::{
    error::{Error, Result},
    tigergraph::{
        edge::{Edge, ProofRecord},
        vertex::{IdentityLoadFn, IdentityRecord},
    },
    upstream::DataFetcher,
    util::make_http_client,
};

use async_graphql::{Context, Object};
use dataloader::non_cached::Loader;
use uuid::Uuid;

#[Object]
impl ProofRecord {
    /// UUID of this record. Generated by us to provide a better
    /// global-uniqueness for future P2P-network data exchange
    /// scenario.
    async fn uuid(&self) -> String {
        self.uuid.to_string()
    }

    /// Data source (upstream) which provides this connection info.
    async fn source(&self) -> String {
        self.source.to_string()
    }

    /// ID of this connection in upstream platform to locate (if any).
    async fn record_id(&self) -> Option<String> {
        self.record_id.clone()
    }

    /// When this connection is recorded in upstream platform (if platform gives such data).
    async fn created_at(&self) -> Option<i64> {
        self.created_at.map(|ca| ca.timestamp())
    }

    /// When this connection is fetched by us RelationService.
    async fn updated_at(&self) -> i64 {
        self.updated_at.timestamp()
    }

    /// Who collects this data.
    /// It works as a "data cleansing" or "proxy" between `source`s and us.
    async fn fetcher(&self) -> DataFetcher {
        self.fetcher
    }

    /// Which `IdentityRecord` does this connection starts at.
    async fn from(&self, ctx: &Context<'_>) -> Result<IdentityRecord> {
        let loader: &Loader<String, Option<IdentityRecord>, IdentityLoadFn> =
            ctx.data().map_err(|err| Error::GraphQLError(err.message))?;
        match loader.load(self.from_id.clone()).await {
            Some(value) => Ok(value),
            None => Err(Error::GraphQLError("record from no found.".to_string())),
        }
    }

    /// Which `IdentityRecord` does this connection ends at.
    async fn to(&self, ctx: &Context<'_>) -> Result<IdentityRecord> {
        let loader: &Loader<String, Option<IdentityRecord>, IdentityLoadFn> =
            ctx.data().map_err(|err| Error::GraphQLError(err.message))?;
        match loader.load(self.to_id.clone()).await {
            Some(value) => Ok(value),
            None => Err(Error::GraphQLError("record to no found.".to_string())),
        }
    }
}

/// Query entrypoint for `Proof{Record}`
#[derive(Default)]
pub struct ProofQuery;

#[Object]
impl ProofQuery {
    async fn proof(
        &self,
        _ctx: &Context<'_>,
        #[graphql(desc = "UUID of this proof")] uuid: Option<String>,
    ) -> Result<Option<ProofRecord>> {
        let client = make_http_client();

        if uuid.is_none() {
            return Ok(None);
        }
        let uuid = Uuid::parse_str(&uuid.unwrap())?;
        let found = ProofRecord::find_by_uuid(&client, &uuid).await?;

        Ok(found)
    }

    /// Prefetch proofs which are prefetchable, e.g. SybilList.
    async fn prefetch_proof(&self) -> Result<String> {
        tokio::spawn(async move {
            let _ = crate::upstream::prefetch().await;
        });
        Ok("Fetching".into())
    }
}
