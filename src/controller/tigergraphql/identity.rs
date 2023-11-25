use crate::{
    error::{Error, Result},
    tigergraph::{
        delete_vertex_and_edge,
        edge::{resolve::ResolveReverse, EdgeUnion, HoldRecord},
        vertex::{Identity, IdentityRecord, IdentityWithSource, OwnerLoadFn},
    },
    upstream::{fetch_all, ContractCategory, DataSource, Platform, Target},
    util::make_http_client,
};

use async_graphql::{Context, Object};
use dataloader::non_cached::Loader;
use strum::IntoEnumIterator;
use tokio::time::{sleep, Duration};
use tracing::{event, Level};
use uuid::Uuid;

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

    async fn reverse(&self) -> Option<bool> {
        self.reverse.clone()
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
        if !self.v_id().is_empty() {
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
    async fn uuid(&self) -> Option<Uuid> {
        self.uuid
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

    /// Uid on target platform.
    /// uid is the unique ID on each platform
    /// e.g. for `Farcaster`, this is the `fid`, for `Lens` this is the lens profile_id(0xabcd)
    async fn uid(&self) -> Option<String> {
        self.uid.clone()
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
        _ctx: &Context<'_>,
        // #[graphql(
        //     desc = "Upstream source of this connection. Will search all upstreams if omitted."
        // )]
        // upstream: Option<String>,
        #[graphql(desc = "Depth of traversal. 1 if omitted")] depth: Option<u16>,
        #[graphql(
            desc = "This reverse flag can be used as a filtering for Identity which type is domain system .
        If `reverse=None` if omitted, there is no need to filter anything.
        When `reverse=true`, just return `primary domain` related identities.
        When `reverse=false`, Only `non-primary domain` will be returned, which is the inverse set of reverse=true."
        )]
        reverse: Option<bool>,
    ) -> Result<Vec<IdentityWithSource>> {
        let client = make_http_client();
        self.neighbors(&client, depth.unwrap_or(1), reverse).await
    }

    /// Neighbor identity from current. The entire topology can be restored by return records.
    async fn neighbor_with_traversal(
        &self,
        _ctx: &Context<'_>,
        #[graphql(desc = "Depth of traversal. 1 if omitted")] depth: Option<u16>,
    ) -> Result<Vec<EdgeUnion>> {
        let client = make_http_client();
        self.neighbors_with_traversal(&client, depth.unwrap_or(1))
            .await
    }

    /// Return primary domain names where they would typically only show addresses.
    async fn reverse_records(&self, _ctx: &Context<'_>) -> Result<Vec<ResolveReverse>> {
        let client = make_http_client();
        self.resolve_reverse_domains(&client).await
    }

    /// there's only `platform: lens, dotbit, unstoppabledomains, farcaster, space_id` identity `ownedBy` is not null
    async fn owned_by(&self, ctx: &Context<'_>) -> Result<Option<IdentityRecord>> {
        if !vec![
            Platform::Lens,
            Platform::Dotbit,
            Platform::UnstoppableDomains,
            Platform::Farcaster,
            Platform::SpaceId,
        ]
        .contains(&self.platform)
        {
            return Ok(None);
        }

        let loader: &Loader<String, Option<IdentityRecord>, OwnerLoadFn> =
            ctx.data().map_err(|err| Error::GraphQLError(err.message))?;

        match loader.load(self.v_id.clone()).await {
            Some(value) => Ok(Some(value)),
            None => Err(Error::GraphQLError("Record not found.".to_string())),
        }
    }
    /// NFTs owned by this identity.
    /// For now, there's only `platform: ethereum` identity has NFTs.
    /// If `category` is provided, only NFTs of that category will be returned.
    async fn nft(
        &self,
        _ctx: &Context<'_>,
        #[graphql(
            desc = "Filter condition for ContractCategory. If not provided or empty array, all category NFTs will be returned."
        )]
        category: Option<Vec<ContractCategory>>,
        #[graphql(
            desc = "`limit` used to control the maximum number of records returned by query. It defaults to 100"
        )]
        limit: Option<u16>,
        #[graphql(
            desc = "`offset` determines the starting position from which the records are retrieved in query. It defaults to 0."
        )]
        offset: Option<u16>,
    ) -> Result<Vec<HoldRecord>> {
        let client = make_http_client();
        self.nfts(&client, category, limit.unwrap_or(100), offset.unwrap_or(0))
            .await
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
    #[tracing::instrument(level = "trace", skip(self, _ctx))]
    async fn identity(
        &self,
        _ctx: &Context<'_>,
        #[graphql(desc = "Platform to query")] platform: String,
        #[graphql(desc = "Identity on target Platform")] identity: String,
    ) -> Result<Option<IdentityRecord>> {
        let client = make_http_client();

        let platform: Platform = platform.parse()?;
        let target = Target::Identity(platform, identity.clone());
        // FIXME: Still kinda dirty. Should be in an background queue/worker-like shape.
        match Identity::find_by_platform_identity(&client, &platform, &identity).await? {
            None => {
                let fetch_result = fetch_all(vec![target], Some(3)).await;
                if fetch_result.is_err() {
                    event!(
                        Level::WARN,
                        ?platform,
                        identity,
                        err = fetch_result.unwrap_err().to_string(),
                        "Failed to fetch"
                    );
                }
                Ok(Identity::find_by_platform_identity(&client, &platform, &identity).await?)
            }
            Some(found) => {
                if found.is_outdated() {
                    event!(
                        Level::DEBUG,
                        ?platform,
                        identity,
                        "Outdated. Delete and Refetching."
                    );
                    let v_id = found.v_id.clone();
                    tokio::spawn(async move {
                        // Delete and Refetch in the background
                        sleep(Duration::from_secs(10)).await;
                        delete_vertex_and_edge(&client, v_id).await?;
                        fetch_all(vec![target], Some(3)).await?;
                        Ok::<_, Error>(())
                    });
                }
                Ok(Some(found))
            }
        }
    }
}
