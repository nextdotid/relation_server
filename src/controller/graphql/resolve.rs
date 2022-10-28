use crate::{
    error::{Error, Result},
    graph::{
        edge::{
            resolve::{DomainNameSystem, DotbitResolve, EnsResolve},
            Resolve,
        },
        vertex::{
            contract::{Chain, ContractCategory, ContractRecord},
            IdentityRecord,
        },
        ConnectionPool,
    },
    upstream::{fetch_all, DataFetcher, DataSource, Platform, Target},
};
use async_graphql::{Context, Object};
use strum::IntoEnumIterator;
use tracing::debug;
use uuid::Uuid;

#[Object]
impl EnsResolve {
    /// UUID of this record.
    async fn uuid(&self) -> Uuid {
        // self.uuid
        self.record.uuid
    }

    /// Data source (upstream) which provides this info.
    async fn source(&self) -> DataSource {
        self.source
    }

    /// Domain Name system
    async fn system(&self) -> DomainNameSystem {
        self.system
    }

    /// Name of domain (e.g., `vitalik.eth`, `dotbit.bit`)
    async fn name(&self) -> String {
        self.name.clone()
    }

    /// Who collects this data.
    /// It works as a "data cleansing" or "proxy" between `source`s and us.
    async fn fetcher(&self) -> DataFetcher {
        self.fetcher
    }

    /// When this connection is fetched by us RelationService.
    async fn updated_at(&self) -> i64 {
        self.updated_at.timestamp()
    }

    /// `resolved`: Find an Ethereum wallet using ENS name.
    async fn resolved(&self) -> Result<ContractRecord> {
        match self.resolved.clone() {
            None => Err(Error::GraphQLError("ENS resolved no found.".to_string())),
            Some(resolved) => Ok(resolved),
        }
    }

    /// `owner`: Return ENS name owned by wallet address.
    async fn owner(&self) -> Result<IdentityRecord> {
        match self.owner.clone() {
            None => Err(Error::GraphQLError("ENS owner no found.".to_string())),
            Some(owner) => Ok(owner),
        }
    }
}

#[Object]
impl DotbitResolve {
    /// UUID of this record.
    async fn uuid(&self) -> Uuid {
        self.uuid
    }

    /// Data source (upstream) which provides this info.
    async fn source(&self) -> DataSource {
        self.source
    }

    /// Domain Name system
    async fn system(&self) -> DomainNameSystem {
        self.system
    }

    /// Name of domain (e.g., `vitalik.eth`, `dotbit.bit`)
    async fn name(&self) -> String {
        self.name.clone()
    }

    /// Who collects this data.
    /// It works as a "data cleansing" or "proxy" between `source`s and us.
    async fn fetcher(&self) -> DataFetcher {
        self.fetcher
    }

    /// When this connection is fetched by us RelationService.
    async fn updated_at(&self) -> i64 {
        self.updated_at.timestamp()
    }

    /// `resolved`: Find an Ethereum wallet using `.bit alias`.
    async fn resolved(&self) -> Result<IdentityRecord> {
        match self.resolved.clone() {
            None => Err(Error::GraphQLError(".bit resolved no found.".to_string())),
            Some(resolved) => Ok(resolved),
        }
    }

    /// `owner`: Return `.bit alias` owned/managed by wallet address.
    async fn owner(&self) -> Result<IdentityRecord> {
        match self.owner.clone() {
            None => Err(Error::GraphQLError(".bit owner no found.".to_string())),
            Some(owner) => Ok(owner),
        }
    }
}

#[derive(Default)]
pub struct ResolveQuery {}

#[Object]
impl ResolveQuery {
    async fn available_name_system(&self) -> Vec<String> {
        DomainNameSystem::iter()
            .map(|system| system.to_string())
            .collect()
    }

    async fn ens(
        &self,
        ctx: &Context<'_>,
        #[graphql(desc = "Name of ENS. For example the name is (name: \"abc.eth\")")] name: String,
    ) -> Result<Option<EnsResolve>> {
        let pool: &ConnectionPool = ctx.data().map_err(|err| Error::PoolError(err.message))?;
        debug!("Connection pool status: {:?}", pool.status());

        let target = Target::NFT(
            Chain::Ethereum,
            ContractCategory::ENS,
            ContractCategory::ENS.default_contract_address().unwrap(),
            name.clone(),
        );
        match Resolve::find_by_ens_name(&pool, &name).await? {
            None => {
                fetch_all(target).await?;
                Resolve::find_by_ens_name(&pool, &name).await
            }
            Some(resolve) => {
                if resolve.is_outdated() {
                    tokio::spawn(fetch_all(target));
                }
                Ok(Some(resolve))
            }
        }
    }

    async fn dotbit(
        &self,
        ctx: &Context<'_>,
        #[graphql(desc = "Name of .bit For example the name is (name: \"abc.bit\")")] name: String,
    ) -> Result<Option<DotbitResolve>> {
        let pool: &ConnectionPool = ctx.data().map_err(|err| Error::PoolError(err.message))?;
        debug!("Connection pool status: {:?}", pool.status());

        let target = Target::Identity(Platform::Dotbit, name.clone());
        match Resolve::find_by_dotbit_name(&pool, &name).await? {
            None => {
                fetch_all(target).await?;
                Resolve::find_by_dotbit_name(&pool, &name).await
            }
            Some(resolve) => {
                if resolve.is_outdated() {
                    tokio::spawn(fetch_all(target));
                }
                Ok(Some(resolve))
            }
        }
    }
}
