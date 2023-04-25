use crate::{
    error::Error,
    graph::edge::{Hold, HoldRecord},
    graph::vertex::{Identity, IdentityRecord},
    graph::{ConnectionPool, Edge},
    upstream::{DataFetcher, DataSource, Platform},
    util::naive_now,
};
use aragog::{
    query::{Comparison, Filter, QueryResult},
    AqlQuery, DatabaseAccess, DatabaseConnection, DatabaseRecord, EdgeRecord, Record,
};
use chrono::{Duration, NaiveDateTime};
use serde::{Deserialize, Serialize};
use strum_macros::{Display, EnumIter, EnumString};
use uuid::Uuid;

#[derive(
    Default,
    Clone,
    Copy,
    Serialize,
    Deserialize,
    Debug,
    Display,
    PartialEq,
    Eq,
    async_graphql::Enum,
    EnumString,
    EnumIter,
    Hash,
)]
pub enum DomainNameSystem {
    /// ENS name system on the ETH chain.
    /// https://ens.domains
    #[strum(serialize = "ENS")]
    #[serde(rename = "ENS")]
    #[graphql(name = "ENS")]
    ENS,

    /// https://www.did.id/
    #[strum(serialize = "dotbit")]
    #[serde(rename = "dotbit")]
    #[graphql(name = "dotbit")]
    DotBit,

    /// https://api.lens.dev/playground
    #[strum(serialize = "lens")]
    #[serde(rename = "lens")]
    #[graphql(name = "lens")]
    Lens,

    /// https://unstoppabledomains.com/
    #[strum(serialize = "unstoppabledomains")]
    #[serde(rename = "unstoppabledomains")]
    #[graphql(name = "unstoppabledomains")]
    UnstoppableDomains,

    /// https://api.prd.space.id/
    #[strum(serialize = "space_id")]
    #[serde(rename = "space_id")]
    #[graphql(name = "space_id")]
    SpaceId,

    #[default]
    #[strum(serialize = "unknown")]
    #[serde(rename = "unknown")]
    #[graphql(name = "unknown")]
    Unknown,
}

impl From<DomainNameSystem> for Platform {
    fn from(domain: DomainNameSystem) -> Self {
        match domain {
            DomainNameSystem::DotBit => Platform::Dotbit,
            DomainNameSystem::UnstoppableDomains => Platform::UnstoppableDomains,
            DomainNameSystem::Lens => Platform::Lens,
            DomainNameSystem::SpaceId => Platform::SpaceId,
            _ => Platform::Unknown,
        }
    }
}

/// Edge to identify which `Identity(Ethereum)` a `Contract` is resolving to.
/// Basiclly this is served for `ENS` only.
/// There're 3 kinds of relation between an `Identity(Ethereum)` and `Contract(ENS)` :
/// - `Own` relation: defined in `graph/edge/own.rs`
///   In our system: create `Own` edge from `Identity(Ethereum)` to `Contract(ENS)`.
/// - `Resolve` relation: Find an Ethereum wallet using ENS name (like DNS).
///   In our system: create `Resolve` edge from `Contract(ENS)` to `Identity(Ethereum)`.
/// - `ReverseLookup` relation: Find an ENS name using an Ethereum wallet (like reverse DNS lookup).
///   In our system: set `display_name` for the `Identity(Ethereum)`.
#[derive(Clone, Serialize, Deserialize, Record, Debug)]
#[collection_name = "Resolves"]
pub struct Resolve {
    /// UUID of this record. Generated by us to provide a better
    /// global-uniqueness for future P2P-network data exchange
    /// scenario.
    pub uuid: Uuid,
    /// Data source (upstream) which provides this connection info.
    pub source: DataSource,
    /// Domain Name system
    pub system: DomainNameSystem,
    /// Name of domain (e.g., `vitalik.eth`)
    pub name: String,
    /// Who collects this data.
    /// It works as a "data cleansing" or "proxy" between `source`s and us.
    pub fetcher: DataFetcher,
    /// When this connection is fetched by us RelationService.
    pub updated_at: NaiveDateTime,
}

impl Default for Resolve {
    fn default() -> Self {
        Self {
            uuid: Default::default(),
            source: Default::default(),
            name: Default::default(),
            system: Default::default(),
            fetcher: Default::default(),
            updated_at: naive_now(),
        }
    }
}

impl Resolve {
    pub async fn find_by_name_system(
        db: &DatabaseConnection,
        name: &str,
        system: &DomainNameSystem,
    ) -> Result<Option<ResolveRecord>, Error> {
        let filter = Filter::new(Comparison::field("system").equals_str(system))
            .and(Comparison::field("name").equals_str(name));
        let query = EdgeRecord::<Self>::query().filter(filter);
        let result: QueryResult<EdgeRecord<Self>> = query.call(db).await?;

        if result.len() == 0 {
            Ok(None)
        } else {
            Ok(Some(result.first().unwrap().clone().into()))
        }
    }

    pub async fn find_by_ens_name(
        pool: &ConnectionPool,
        name: &str,
    ) -> Result<Option<ResolveEdge>, Error> {
        let conn = pool
            .get()
            .await
            .map_err(|err| Error::PoolError(err.to_string()))?;
        let db = conn.database();

        let aql_str = r###"
            FOR r IN @@resolves
                FILTER r.system == @system AND 
                r.name == @name AND
                CONTAINS(r._from, "Identities") AND
                CONTAINS(r._to, "Contracts")
                LET resolved = FIRST(FOR c IN @@identities FILTER c._id == r._from RETURN c)
            FOR h IN @@holds
                FILTER h.id == @name
                LET owner = FIRST(FOR c IN @@identities FILTER c._id == h._from RETURN c)
            RETURN {"record": r, "resolved": resolved, "owner": owner}"###;

        let aql = AqlQuery::new(aql_str)
            .bind_var("@resolves", Resolve::COLLECTION_NAME)
            .bind_var("@holds", Hold::COLLECTION_NAME)
            .bind_var("@identities", Identity::COLLECTION_NAME)
            .bind_var("system", DomainNameSystem::ENS.to_string())
            .bind_var("name", name.clone())
            .batch_size(1)
            .count(false);

        let result: Vec<ResolveEdge> = db.aql_query(aql).await?;
        if result.len() == 0 {
            let aql_str = r###"
            FOR h IN @@holds FILTER h.id == @name
                LET owner = FIRST(FOR c IN @@identities FILTER c._id == h._from RETURN c)
            RETURN {"record": h, "owner": owner}"###;

            let aql = AqlQuery::new(aql_str)
                .bind_var("@holds", Hold::COLLECTION_NAME)
                .bind_var("@identities", Identity::COLLECTION_NAME)
                .bind_var("name", name.clone())
                .batch_size(1)
                .count(false);

            let res: Vec<HoldEdge> = db.aql_query(aql).await?;
            if res.len() > 0 {
                let r = res.first().unwrap().to_owned();
                let mut resolve_edge = ResolveEdge::from(Resolve {
                    uuid: r.record.uuid,
                    source: r.record.source,
                    system: DomainNameSystem::ENS,
                    name: name.to_string(),
                    fetcher: r.record.fetcher,
                    updated_at: r.record.updated_at,
                });
                resolve_edge.owner = res.first().unwrap().to_owned().owner;
                resolve_edge.resolved = None;
                Ok(Some(resolve_edge))
            } else {
                Ok(None)
            }
        } else {
            Ok(Some(result.first().unwrap().to_owned().into()))
        }
    }

    pub async fn find_by_domain_platform_name(
        pool: &ConnectionPool,
        name: &str,
        domain_system: &DomainNameSystem,
        platform: &Platform,
    ) -> Result<Option<ResolveEdge>, Error> {
        let conn = pool
            .get()
            .await
            .map_err(|err| Error::PoolError(err.to_string()))?;
        let db = conn.database();

        let aql = r###"
        FOR r IN @@resolves
            FILTER r.system == @system AND r.name == @name
            LET resolved = FIRST(FOR c IN @@identities FILTER c._id == r._to RETURN c)
        LET owner = FIRST(FOR i IN @@identities
            FILTER i.platform == @platform AND i.identity == @identity
            LIMIT 1
            FOR vertex, edge, path
                IN 1..1 ANY i @@holds
                FILTER NOT CONTAINS(path.edges[*]._to, "Contracts")
                RETURN DISTINCT vertex
            )
        RETURN {"record": r, "resolved": resolved, "owner": owner}"###;

        let aql = AqlQuery::new(aql)
            .bind_var("@resolves", Resolve::COLLECTION_NAME)
            .bind_var("@holds", Hold::COLLECTION_NAME)
            .bind_var("@identities", Identity::COLLECTION_NAME)
            .bind_var("system", domain_system.to_string())
            .bind_var("name", name.clone())
            .bind_var("platform", platform.to_string())
            .bind_var("identity", name.clone())
            .batch_size(1)
            .count(false);

        let result: Vec<ResolveEdge> = db.aql_query(aql).await?;
        if result.len() == 0 {
            let aql_str = r###"
            FOR i IN @@identities
            FILTER i.platform == @platform AND i.identity == @identity
            LIMIT 1
            FOR vertex, edge, path
                IN 1..1 ANY i @@holds
                FILTER path.edges[*].source ALL == @platform
                RETURN {"record": edge, "owner": i}"###;

            let aql = AqlQuery::new(aql_str)
                .bind_var("@holds", Hold::COLLECTION_NAME)
                .bind_var("@identities", Identity::COLLECTION_NAME)
                .bind_var("platform", platform.to_string())
                .bind_var("identity", name.clone())
                .batch_size(1)
                .count(false);

            let res: Vec<HoldEdge> = db.aql_query(aql).await?;
            if res.len() > 0 {
                let record = res.first().unwrap().to_owned().record;
                let mut resolve_edge = ResolveEdge::from(Resolve {
                    uuid: record.uuid,
                    source: record.source,
                    system: DomainNameSystem::DotBit,
                    name: res
                        .first()
                        .unwrap()
                        .to_owned()
                        .owner
                        .unwrap()
                        .identity
                        .clone(),
                    fetcher: record.fetcher,
                    updated_at: record.updated_at,
                });
                resolve_edge.owner = res.first().unwrap().to_owned().owner;
                resolve_edge.resolved = None;
                Ok(Some(resolve_edge))
            } else {
                Ok(None)
            }
        } else {
            Ok(Some(result.first().unwrap().to_owned().into()))
        }
    }

    pub fn is_outdated(&self) -> bool {
        let outdated_in = Duration::days(1);
        self.updated_at
            .checked_add_signed(outdated_in)
            .unwrap()
            .lt(&naive_now())
    }
}

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct ResolveEdge {
    pub record: Resolve,
    pub resolved: Option<IdentityRecord>,
    pub owner: Option<IdentityRecord>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct HoldEdge {
    pub record: HoldRecord,
    pub owner: Option<IdentityRecord>,
}

impl std::ops::Deref for ResolveEdge {
    type Target = Resolve;

    fn deref(&self) -> &Self::Target {
        &self.record
    }
}
impl std::ops::DerefMut for ResolveEdge {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.record
    }
}
impl From<Resolve> for ResolveEdge {
    fn from(record: Resolve) -> Self {
        ResolveEdge {
            record,
            resolved: None,
            owner: None,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct ResolveRecord(DatabaseRecord<EdgeRecord<Resolve>>);
impl std::ops::Deref for ResolveRecord {
    type Target = DatabaseRecord<EdgeRecord<Resolve>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for ResolveRecord {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<DatabaseRecord<EdgeRecord<Resolve>>> for ResolveRecord {
    fn from(record: DatabaseRecord<EdgeRecord<Resolve>>) -> Self {
        ResolveRecord(record)
    }
}

#[async_trait::async_trait]
impl<T1: Record + std::marker::Sync, T2: Record + std::marker::Sync> Edge<T1, T2, ResolveRecord>
    for Resolve
{
    fn uuid(&self) -> Option<Uuid> {
        Some(self.uuid)
    }

    async fn connect(
        &self,
        db: &DatabaseConnection,
        from: &DatabaseRecord<T1>,
        to: &DatabaseRecord<T2>,
    ) -> Result<ResolveRecord, Error> {
        let filter = Filter::new(Comparison::field("_from").equals_str(from.id()))
            .and(Comparison::field("_to").equals_str(to.id()))
            .and(Comparison::field("system").equals_str(&self.system))
            .and(Comparison::field("name").equals_str(&self.name));
        let query = EdgeRecord::<Resolve>::query().filter(filter);
        let result: QueryResult<EdgeRecord<Self>> = query.call(db).await?;
        if result.len() == 0 {
            Ok(DatabaseRecord::link(from, to, db, self.clone())
                .await?
                .into())
        } else {
            Ok(result.first().unwrap().clone().into())
        }
    }

    /*
    async fn connect(
        &self,
        db: &DatabaseConnection,
        from: &DatabaseRecord<T>,
        to: &DatabaseRecord<Identity>,
    ) -> Result<ResolveRecord, Error> {
        let found = Self::find_by_name_system(db, &self.name, &self.system).await?;
        match found {
            Some(mut edge) => {
                if edge.key_from() == from.key() && edge.key_to() == to.key() {
                    // Exact the same edge. Keep it.
                    Ok(edge)
                } else {
                    // Destory old edge and create new one.
                    edge.delete(db).await?;
                    Ok(DatabaseRecord::link(from, to, db, self.clone())
                        .await?
                        .into())
                }
            }
            None => Ok(DatabaseRecord::link(from, to, db, self.clone())
                    .await?
                    .into()),
        }
    }
    */

    // notice this function is deprecated
    async fn two_way_binding(
        &self,
        _db: &DatabaseConnection,
        _from: &DatabaseRecord<T1>,
        _to: &DatabaseRecord<T2>,
    ) -> Result<(ResolveRecord, ResolveRecord), Error> {
        todo!()
    }

    async fn find_by_uuid(
        db: &DatabaseConnection,
        uuid: &Uuid,
    ) -> Result<Option<ResolveRecord>, Error> {
        let result: QueryResult<EdgeRecord<Self>> = EdgeRecord::<Self>::query()
            .filter(Comparison::field("uuid").equals_str(uuid).into())
            .call(db)
            .await?;

        if result.len() == 0 {
            Ok(None)
        } else {
            Ok(Some(result.first().unwrap().to_owned().into()))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::error::Error;
    use crate::graph::arangopool::new_connection_pool;
    use crate::graph::edge::resolve::DomainNameSystem;
    use crate::graph::edge::Resolve;
    use crate::upstream::Platform;

    #[tokio::test]
    async fn test_find_by_ens_name() -> Result<(), Error> {
        let pool = new_connection_pool().await?;
        let name = "zzfzz.eth";
        let a = Resolve::find_by_ens_name(&pool, &name).await?;
        print!("result: {:?}", a);
        Ok(())
    }

    #[tokio::test]
    async fn test_find_by_dotbit_name() -> Result<(), Error> {
        let pool = new_connection_pool().await?;
        let name = "thefuzezhong.bit";
        let a = Resolve::find_by_domain_platform_name(
            &pool,
            &name,
            &DomainNameSystem::DotBit,
            &Platform::Dotbit,
        )
        .await?;
        print!("result: {:?}", a);
        Ok(())
    }
}
