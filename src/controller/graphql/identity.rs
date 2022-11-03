use crate::controller::vec_string_to_vec_platform;
use crate::error::{Error, Result};
use crate::graph::edge::{HoldRecord, IdentityFromToRecord};
use crate::graph::vertex::contract::ContractCategory;
use crate::graph::vertex::{Identity, IdentityRecord, IdentityWithSource, Vertex};
use crate::graph::ConnectionPool;
use crate::upstream::{fetch_all, DataSource, Platform, Target};
use async_graphql::{Context, Object};
use deadpool::managed::Object;
use strum::IntoEnumIterator;
use tracing::{debug, info};

/// Status for a record in RelationService DB
#[derive(Default, Copy, Clone, PartialEq, Eq, async_graphql::Enum)]
enum DataStatus {
    /// Fetched or not in Database.
    #[default]
    #[graphql(name = "cached")]
    Cached,

    /// Outdated record
    #[graphql(name = "outdated")]
    Outdated,

    /// Fetching this data.
    /// The result you got maybe outdated.
    /// Come back later if you want a fresh one.
    #[graphql(name = "fetching")]
    Fetching,
}

#[Object]
impl IdentityWithSource {
    async fn sources(&self) -> Vec<DataSource> {
        self.sources.clone()
    }

    async fn identity(&self) -> IdentityRecord {
        self.identity.clone()
    }
}

#[Object]
impl IdentityRecord {
    /// Status for this record in RelationService.
    async fn status(&self) -> Vec<DataStatus> {
        use DataStatus::*;
        let mut current: Vec<DataStatus> = vec![];
        if !self.key().is_empty() {
            current.push(Cached);
            if self.is_outdated() {
                current.push(Outdated);
            }
        } else {
            current.push(Fetching); // FIXME: Seems like this is never reached.
        }
        current
    }

    /// UUID of this record.  Generated by us to provide a better
    /// global-uniqueness for future P2P-network data exchange
    /// scenario.
    async fn uuid(&self) -> Option<String> {
        self.uuid.map(|u| u.to_string())
    }

    /// Platform.  See `avaliablePlatforms` or schema definition for a
    /// list of platforms supported by RelationService.
    async fn platform(&self) -> Platform {
        self.platform
    }

    /// Identity on target platform.  Username or database primary key
    /// (prefer, usually digits).  e.g. `Twitter` has this digits-like
    /// user ID thing.
    async fn identity(&self) -> String {
        self.identity.clone()
    }

    /// Usually user-friendly screen name.  e.g. for `Twitter`, this
    /// is the user's `screen_name`.
    /// Note: both `null` and `""` should be treated as "no value".
    async fn display_name(&self) -> Option<String> {
        self.display_name.clone()
    }

    /// URL to target identity profile page on `platform` (if any).
    async fn profile_url(&self) -> Option<String> {
        self.profile_url.clone()
    }

    /// URL to avatar (if any is recorded and given by target platform).
    async fn avatar_url(&self) -> Option<String> {
        self.avatar_url.clone()
    }

    /// Account / identity creation time ON TARGET PLATFORM.
    /// This is not necessarily the same as the creation time of the record in the database.
    /// Since `created_at` may not be recorded or given by target platform.
    /// e.g. `Twitter` has a `created_at` in the user profile API.
    /// but `Ethereum` is obviously no such thing.
    async fn created_at(&self) -> Option<i64> {
        self.created_at.map(|dt| dt.timestamp())
    }

    /// When this Identity is added into this database.
    /// Second-based unix timestamp.
    /// Generated by us.
    async fn added_at(&self) -> i64 {
        self.added_at.timestamp()
    }

    /// When it is updated (re-fetched) by us RelationService.
    /// Second-based unix timestamp.
    /// Managed by us.
    async fn updated_at(&self) -> i64 {
        self.updated_at.timestamp()
    }

    /// Neighbor identity from current. Flattened.
    async fn neighbor(
        &self,
        ctx: &Context<'_>,
        // #[graphql(
        //     desc = "Upstream source of this connection. Will search all upstreams if omitted."
        // )]
        // upstream: Option<String>,
        #[graphql(desc = "Depth of traversal. 1 if omitted")] depth: Option<u16>,
    ) -> Result<Vec<IdentityWithSource>> {
        let pool: &ConnectionPool = ctx.data().map_err(|err| Error::PoolError(err.message))?;
        debug!("Connection pool status: {:?}", pool.status());

        self.neighbors(
            pool,
            depth.unwrap_or(1),
            // upstream.map(|u| DataSource::from_str(&u).unwrap_or(DataSource::Unknown))
            None,
        )
        .await
    }

    async fn neighbor_with_traversal(
        &self,
        ctx: &Context<'_>,
        #[graphql(desc = "Depth of traversal. 1 if omitted")] depth: Option<u16>,
    ) -> Result<Vec<IdentityFromToRecord>> {
        let pool: &ConnectionPool = ctx.data().map_err(|err| Error::PoolError(err.message))?;
        debug!("Connection pool status: {:?}", pool.status());
        self.neighbors_with_traversal(pool, depth.unwrap_or(1))
            .await
    }

    /// there's only `platform: lens` identity `ownedBy` is not null
    async fn owned_by(&self, ctx: &Context<'_>) -> Result<Option<IdentityRecord>> {
        if self.platform != Platform::Lens {
            return Ok(None);
        } else {
            let pool: &ConnectionPool = ctx.data().map_err(|err| Error::PoolError(err.message))?;
            debug!("Connection pool status: {:?}", pool.status());
            self.lens_owned_by(pool).await
        }
    }

    /// NFTs owned by this identity.
    /// For now, there's only `platform: ethereum` identity has NFTs.
    /// If `category` is present, result will be filtered by given `category`s.
    async fn nft(
        &self,
        ctx: &Context<'_>,
        category: Option<Vec<ContractCategory>>,
    ) -> Result<Vec<HoldRecord>> {
        let pool: &ConnectionPool = ctx.data().map_err(|err| Error::PoolError(err.message))?;
        debug!("Connection pool status: {:?}", pool.status());
        self.nfts(pool, category).await
    }
}

#[derive(Default)]
pub struct IdentityQuery;

#[Object]
impl IdentityQuery {
    /// Returns a list of all platforms supported by RelationService.
    async fn available_platforms(&self) -> Result<Vec<Platform>> {
        Ok(Platform::iter().collect())
    }

    /// Returns a list of all upstreams (data sources) supported by RelationService.
    async fn available_upstreams(&self) -> Result<Vec<DataSource>> {
        Ok(DataSource::iter().collect())
    }

    /// Query an `identity` by given `platform` and `identity`.
    async fn identity(
        &self,
        ctx: &Context<'_>,
        #[graphql(desc = "Platform to query")] platform: String,
        #[graphql(desc = "Identity on target Platform")] identity: String,
    ) -> Result<Option<IdentityRecord>> {
        // let db: &DatabaseConnection = ctx.data().map_err(|err| Error::GraphQLError(err.message))?;
        let pool: &ConnectionPool = ctx.data().map_err(|err| Error::PoolError(err.message))?;
        debug!("Connection pool status: {:?}", pool.status());

        let conn = pool
            .get()
            .await
            .map_err(|err| Error::PoolError(err.to_string()))?;
        let db = Object::take(conn);

        let platform: Platform = platform.parse()?;
        let target = Target::Identity(platform, identity.clone());
        // FIXME: Still kinda dirty. Should be in an background queue/worker-like shape.
        match Identity::find_by_platform_identity(&db, &platform, &identity).await? {
            None => {
                let _ = fetch_all(target).await; // TODO: print error message here (but not break the return value)
                Ok(Identity::find_by_platform_identity(&db, &platform, &identity).await?)
            }
            Some(found) => {
                if found.is_outdated() {
                    info!(
                        "Identity: {}/{} is outdated. Refetching...",
                        platform, identity
                    );
                    tokio::spawn(fetch_all(target)); // Fetch in the background
                }
                Ok(Some(found))
            }
        }
    }

    async fn identities(
        &self,
        ctx: &Context<'_>,
        #[graphql(desc = "Platform array to query")] platforms: Vec<String>,
        #[graphql(desc = "Identity on target Platform")] identity: String,
    ) -> Result<Vec<IdentityRecord>> {
        let pool: &ConnectionPool = ctx.data().map_err(|err| Error::GraphQLError(err.message))?;
        debug!("Connection pool status: {:?}", pool.status());

        let platform_list = vec_string_to_vec_platform(platforms)?;
        let record: Vec<IdentityRecord> =
            Identity::find_by_platforms_identity(&pool, &platform_list, identity.as_str()).await?;
        if record.len() == 0 {
            for platform in &platform_list {
                let target = Target::Identity(platform.clone(), identity.clone());
                fetch_all(target).await?;
            }
            Identity::find_by_platforms_identity(&pool, &platform_list, identity.as_str()).await
        } else {
            record.iter().filter(|r| r.is_outdated()).for_each(|r| {
                // Refetch in the background
                tokio::spawn(fetch_all(Target::Identity(
                    r.platform.clone(),
                    r.identity.clone(),
                )));
            });
            Ok(record)
        }
    }
}
