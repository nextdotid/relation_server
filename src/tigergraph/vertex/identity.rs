use crate::{
    config::C,
    error::Error,
    tigergraph::{
        edge::{
            resolve::{ResolveRecord, ResolveReverse},
            EdgeUnion, HoldRecord,
        },
        upsert_graph,
        vertex::{FromWithParams, Vertex, VertexRecord},
        Attribute, BaseResponse, Graph, OpCode, Transfer, UpsertGraph, Vertices,
    },
    upstream::{
        vec_string_to_vec_datasource, ContractCategory, DataSource, DomainNameSystem, Platform,
    },
    util::{
        naive_datetime_from_string, naive_datetime_to_string, naive_now,
        option_naive_datetime_from_string, option_naive_datetime_to_string, parse_body,
    },
};

use async_trait::async_trait;
use chrono::{Duration, NaiveDateTime};
use dataloader::BatchFn;
use http::uri::InvalidUri;
use hyper::{client::HttpConnector, Body, Client, Method};
use serde::de::{self, Deserializer, MapAccess, Visitor};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::fmt;
use tracing::{error, trace};
use uuid::Uuid;

pub const VERTEX_NAME: &str = "Identities";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Identity {
    /// UUID of this record. Generated by us to provide a better
    /// global-uniqueness for future P2P-network data exchange
    /// scenario.
    pub uuid: Option<Uuid>,
    /// Platform.
    pub platform: Platform,
    /// Identity on target platform.
    /// Username or database primary key (prefer, usually digits).
    /// e.g. `Twitter` has this digits-like user ID thing.
    pub identity: String,
    /// Uid on target platform.
    /// uid is the unique ID on each platform
    /// e.g. for `Farcaster`, this is the `fid`, for `Lens` this is the lens profile_id(0xabcd)
    pub uid: Option<String>,
    /// Usually user-friendly screen name.
    /// e.g. for `Twitter`, this is the user's `screen_name`.
    /// For `ethereum`, this is the reversed ENS name set by user.
    pub display_name: Option<String>,
    /// URL to target identity profile page on `platform` (if any).
    pub profile_url: Option<String>,
    /// URL to avatar (if any is recorded and given by target platform).
    pub avatar_url: Option<String>,
    /// Account / identity creation time ON TARGET PLATFORM.
    /// This is not necessarily the same as the creation time of the record in the database.
    /// Since `created_at` may not be recorded or given by target platform.
    /// e.g. `Twitter` has a `created_at` in the user profile API.
    /// but `Ethereum` is obviously no such thing.
    #[serde(deserialize_with = "option_naive_datetime_from_string")]
    #[serde(serialize_with = "option_naive_datetime_to_string")]
    pub created_at: Option<NaiveDateTime>,
    /// When this Identity is added into this database. Generated by us.
    #[serde(deserialize_with = "naive_datetime_from_string")]
    #[serde(serialize_with = "naive_datetime_to_string")]
    pub added_at: NaiveDateTime,
    /// When it is updated (re-fetched) by us RelationService. Managed by us.
    #[serde(deserialize_with = "naive_datetime_from_string")]
    #[serde(serialize_with = "naive_datetime_to_string")]
    pub updated_at: NaiveDateTime,
}

#[async_trait]
impl Vertex for Identity {
    fn primary_key(&self) -> String {
        // self.0.v_id.clone()
        format!("{},{}", self.platform, self.identity)
    }

    fn vertex_type(&self) -> String {
        VERTEX_NAME.to_string()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IdentityRecord(pub VertexRecord<Identity>);

impl FromWithParams<Identity> for IdentityRecord {
    fn from_with_params(v_type: String, v_id: String, attributes: Identity) -> Self {
        IdentityRecord(VertexRecord {
            v_type,
            v_id,
            attributes,
        })
    }
}

impl From<VertexRecord<Identity>> for IdentityRecord {
    fn from(record: VertexRecord<Identity>) -> Self {
        IdentityRecord(record)
    }
}

impl std::ops::Deref for IdentityRecord {
    type Target = VertexRecord<Identity>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for IdentityRecord {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl std::ops::Deref for VertexRecord<Identity> {
    type Target = Identity;

    fn deref(&self) -> &Self::Target {
        &self.attributes
    }
}

impl std::ops::DerefMut for VertexRecord<Identity> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.attributes
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IdentityAttribute(HashMap<String, Attribute>);

// Implement `Transfer` trait for converting `Identity` into a `HashMap<String, Attribute>`.
impl Transfer for Identity {
    fn to_attributes_map(&self) -> HashMap<String, Attribute> {
        let mut attributes_map = HashMap::new();

        attributes_map.insert(
            "id".to_string(),
            Attribute {
                value: json!(self.primary_key()),
                op: Some(OpCode::IgnoreIfExists),
            },
        );
        if let Some(uuid) = self.uuid {
            attributes_map.insert(
                "uuid".to_string(),
                Attribute {
                    value: json!(uuid),
                    op: Some(OpCode::IgnoreIfExists),
                },
            );
        }
        attributes_map.insert(
            "platform".to_string(),
            Attribute {
                value: json!(self.platform),
                op: Some(OpCode::IgnoreIfExists),
            },
        );
        attributes_map.insert(
            "identity".to_string(),
            Attribute {
                value: json!(&self.identity),
                op: Some(OpCode::IgnoreIfExists),
            },
        );
        if let Some(uid) = self.uid.clone() {
            attributes_map.insert(
                "uid".to_string(),
                Attribute {
                    value: json!(uid),
                    op: None,
                },
            );
        }
        if let Some(display_name) = self.display_name.clone() {
            attributes_map.insert(
                "display_name".to_string(),
                Attribute {
                    value: json!(display_name),
                    op: None,
                },
            );
        }
        if let Some(profile_url) = self.profile_url.clone() {
            attributes_map.insert(
                "profile_url".to_string(),
                Attribute {
                    value: json!(profile_url),
                    op: None,
                },
            );
        }
        if let Some(avatar_url) = self.avatar_url.clone() {
            attributes_map.insert(
                "avatar_url".to_string(),
                Attribute {
                    value: json!(avatar_url),
                    op: None,
                },
            );
        }
        if let Some(created_at) = self.created_at {
            attributes_map.insert(
                "created_at".to_string(),
                Attribute {
                    value: json!(created_at),
                    op: Some(OpCode::IgnoreIfExists),
                },
            );
        }

        attributes_map.insert(
            "added_at".to_string(),
            Attribute {
                value: json!(self.added_at),
                op: None,
            },
        );
        attributes_map.insert(
            "updated_at".to_string(),
            Attribute {
                value: json!(self.updated_at),
                op: Some(OpCode::Max),
            },
        );
        attributes_map
    }
}

impl Default for Identity {
    fn default() -> Self {
        Self {
            uuid: Default::default(),
            platform: Platform::Twitter,
            identity: Default::default(),
            uid: Default::default(),
            display_name: Default::default(),
            profile_url: None,
            avatar_url: None,
            created_at: None,
            added_at: naive_now(),
            updated_at: naive_now(),
        }
    }
}

impl PartialEq for Identity {
    fn eq(&self, other: &Self) -> bool {
        self.uuid.is_some() && other.uuid.is_some() && self.uuid == other.uuid
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NeighborsWithSource {
    #[serde(flatten)]
    base: BaseResponse,
    results: Option<Vec<VertexWithSource>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VertexWithSource {
    vertices: Vec<IdentityWithSource>,
}

#[derive(Clone, Serialize, Debug)]
pub struct IdentityWithSource {
    pub identity: IdentityRecord,
    pub sources: Vec<DataSource>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NeighborsResponse {
    #[serde(flatten)]
    base: BaseResponse,
    results: Option<Vec<EdgeUnions>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct EdgeUnions {
    edges: Vec<EdgeUnion>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReverseDomainsResponse {
    #[serde(flatten)]
    base: BaseResponse,
    results: Option<Vec<ReverseRecords>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReverseRecords {
    reverse_records: Vec<ResolveRecord>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OwnedByResponse {
    #[serde(flatten)]
    base: BaseResponse,
    results: Option<Vec<Owner>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Owner {
    owner: Vec<IdentityRecord>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct QueryNftsResponse {
    #[serde(flatten)]
    base: BaseResponse,
    results: Option<Vec<Nfts>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Nfts {
    edges: Vec<HoldRecord>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IdentityBySourceResponse {
    #[serde(flatten)]
    base: BaseResponse,
    results: Option<Vec<Identities>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Identities {
    vertices: Vec<IdentityRecord>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VertexResponse {
    #[serde(flatten)]
    base: BaseResponse,
    results: Option<Vec<IdentityRecord>>,
}

impl<'de> Deserialize<'de> for IdentityWithSource {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct IdentityWithSourceVisitor;
        impl<'de> Visitor<'de> for IdentityWithSourceVisitor {
            type Value = IdentityWithSource;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("struct IdentityWithSource")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut v_type: Option<String> = None;
                let mut v_id: Option<String> = None;
                let mut attributes: Option<serde_json::Map<String, serde_json::Value>> = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        "v_type" => v_type = Some(map.next_value()?),
                        "v_id" => v_id = Some(map.next_value()?),
                        "attributes" => attributes = Some(map.next_value()?),
                        _ => {}
                    }
                }

                let mut attributes =
                    attributes.ok_or_else(|| de::Error::missing_field("attributes"))?;

                let source_list = attributes
                    .remove("@source_list")
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(de::Error::custom)?;

                let domain_reverse: Option<bool> = attributes
                    .remove("@reverse")
                    .map(serde_json::from_value)
                    .transpose()
                    .map_err(de::Error::custom)?;

                let attributes: Identity =
                    serde_json::from_value(serde_json::Value::Object(attributes))
                        .map_err(de::Error::custom)?;

                let v_type = v_type.ok_or_else(|| de::Error::missing_field("v_type"))?;
                let v_id = v_id.ok_or_else(|| de::Error::missing_field("v_id"))?;
                let source_list =
                    source_list.ok_or_else(|| de::Error::missing_field("@source_list"))?;
                let sources =
                    vec_string_to_vec_datasource(source_list).map_err(de::Error::custom)?;
                let domain_system: DomainNameSystem = attributes.platform.clone().into();
                if domain_system != DomainNameSystem::Unknown {
                    reverse = domain_reverse;
                }

                Ok(IdentityWithSource {
                    identity: IdentityRecord(VertexRecord {
                        v_type,
                        v_id,
                        attributes,
                    }),
                    sources,
                    reverse,
                })
            }
        }

        deserializer.deserialize_map(IdentityWithSourceVisitor)
    }
}

impl Identity {
    pub fn uuid(&self) -> Option<Uuid> {
        self.uuid
    }

    /// Judge if this record is outdated and should be refetched.
    pub fn is_outdated(&self) -> bool {
        let outdated_in = Duration::hours(1);
        self.updated_at
            .checked_add_signed(outdated_in)
            .unwrap()
            .lt(&naive_now())
    }

    /// Create or update a vertex.
    pub async fn create_or_update(&self, client: &Client<HttpConnector>) -> Result<(), Error> {
        let vertices = Vertices(vec![self.to_owned()]);
        let graph = UpsertGraph {
            vertices: vertices.into(),
            edges: None,
        };
        upsert_graph(client, &graph, Graph::IdentityGraph).await?;
        Ok(())
    }

    /// Find a vertex by UUID.
    pub async fn find_by_uuid(
        client: &Client<HttpConnector>,
        uuid: Uuid,
    ) -> Result<Option<IdentityRecord>, Error> {
        // Builtins: http://server:9000/graph/{GraphName}/vertices/{VertexName}/filter=field1="a",field2="b"
        let uri: http::Uri = format!(
            "{}/graph/{}/vertices/{}?filter=uuid=%22{}%22",
            C.tdb.host,
            Graph::IdentityGraph.to_string(),
            VERTEX_NAME,
            uuid.to_string(),
        )
        .parse()
        .map_err(|_err: InvalidUri| Error::ParamError(format!("Uri format Error {}", _err)))?;
        let req = hyper::Request::builder()
            .method(Method::GET)
            .uri(uri)
            .header("Authorization", Graph::IdentityGraph.token())
            .body(Body::empty())
            .map_err(|_err| Error::ParamError(format!("ParamError Error {}", _err)))?;

        let mut resp = client.request(req).await.map_err(|err| {
            Error::ManualHttpClientError(format!(
                "query filter error | Fail to request: {:?}",
                err.to_string()
            ))
        })?;
        match parse_body::<VertexResponse>(&mut resp).await {
            Ok(r) => {
                if r.base.error {
                    let err_message = format!(
                        "TigerGraph query filter error | Code: {:?}, Message: {:?}",
                        r.base.code, r.base.message
                    );
                    error!(err_message);
                    return Err(Error::General(err_message, resp.status()));
                }
                let result: Option<IdentityRecord> = r
                    .results
                    .and_then(|results: Vec<IdentityRecord>| results.first().cloned());
                Ok(result)
            }
            Err(err) => {
                let err_message = format!("TigerGraph query filter parse_body error: {:?}", err);
                error!(err_message);
                return Err(err);
            }
        }
    }

    /// Find `IdentityRecord` by given platform and identity.
    pub async fn find_by_platform_identity(
        client: &Client<HttpConnector>,
        platform: &Platform,
        identity: &str,
    ) -> Result<Option<IdentityRecord>, Error> {
        // Builtins: http://server:9000/graph/{GraphName}/vertices/{VertexName}/filter=field1="a",field2="b"
        let uri: http::Uri = format!(
            "{}/graph/{}/vertices/{}?filter=platform=%22{}%22,identity=%22{}%22",
            C.tdb.host,
            Graph::IdentityGraph.to_string(),
            VERTEX_NAME,
            platform.to_string(),
            identity.to_string(),
        )
        .parse()
        .map_err(|_err: InvalidUri| {
            Error::ParamError(format!(
                "QUERY filter=platform=%22{}%22,identity=%22{}%22 Uri format Error | {}",
                platform.to_string(),
                identity.to_string(),
                _err
            ))
        })?;
        let req = hyper::Request::builder()
            .method(Method::GET)
            .uri(uri)
            .header("Authorization", Graph::IdentityGraph.token())
            .body(Body::empty())
            .map_err(|_err| Error::ParamError(format!("ParamError Error | {}", _err)))?;

        let mut resp = client.request(req).await.map_err(|err| {
            Error::ManualHttpClientError(format!(
                "query filter=platform=%22{}%22,identity=%22{}%22 error | Fail to request: {:?}",
                platform.to_string(),
                identity.to_string(),
                err.to_string()
            ))
        })?;
        match parse_body::<VertexResponse>(&mut resp).await {
            Ok(r) => {
                if r.base.error {
                    let err_message = format!(
                        "TigerGraph query filter error | Code: {:?}, Message: {:?}",
                        r.base.code, r.base.message
                    );
                    error!(err_message);
                    return Err(Error::General(err_message, resp.status()));
                }
                let result: Option<IdentityRecord> = r
                    .results
                    .and_then(|results: Vec<IdentityRecord>| results.first().cloned());
                Ok(result)
            }
            Err(err) => {
                let err_message = format!("TigerGraph query filter parse_body error: {:?}", err);
                error!(err_message);
                return Err(err);
            }
        }
    }
}

impl IdentityRecord {
    pub fn v_id(&self) -> String {
        self.v_id.clone()
    }

    pub fn v_type(&self) -> String {
        self.v_type.clone()
    }

    /// Return all neighbors of this identity with sources.
    pub async fn neighbors(
        &self,
        client: &Client<HttpConnector>,
        depth: u16,
        reverse: Option<bool>,
    ) -> Result<Vec<IdentityWithSource>, Error> {
        // This reverse flag can be used as a filtering for Identity which type is domain system .
        // flag = 0, If `reverse=None` if omitted, there is no need to filter anything.
        // flag = 1, When `reverse=true`, just return `primary domain` related identities.
        // flag = 2, When `reverse=false`, Only `non-primary domain` will be returned, which is the inverse set of reverse=true.
        let flag = reverse.map_or(0, |r| match r {
            true => 1,
            false => 2,
        });
        // query see in Solution: CREATE QUERY neighbors_with_source(VERTEX<Identities> p, INT depth)
        let uri: http::Uri = format!(
            "{}/query/{}/neighbors_with_source_reverse?p={}&depth={}&reverse_flag={}",
            C.tdb.host,
            Graph::IdentityGraph.to_string(),
            self.v_id,
            depth,
            flag,
        )
        .parse()
        .map_err(|_err: InvalidUri| Error::ParamError(format!("Uri format Error {}", _err)))?;

        let req = hyper::Request::builder()
            .method(Method::GET)
            .uri(uri)
            .header("Authorization", Graph::IdentityGraph.token())
            .body(Body::empty())
            .map_err(|_err| Error::ParamError(format!("ParamError Error {}", _err)))?;
        let mut resp = client.request(req).await.map_err(|err| {
            Error::ManualHttpClientError(format!(
                "query neighbors_with_source | Fail to request: {:?}",
                err.to_string()
            ))
        })?;

        match parse_body::<NeighborsWithSource>(&mut resp).await {
            Ok(r) => {
                if r.base.error {
                    let err_message = format!(
                        "TigerGraph query neighbors_with_source error | Code: {:?}, Message: {:?}",
                        r.base.code, r.base.message
                    );
                    error!(err_message);
                    return Err(Error::General(err_message, resp.status()));
                }

                let result: Vec<IdentityWithSource> = r
                    .results
                    .and_then(|vec_with_sources| vec_with_sources.first().cloned())
                    .map_or(vec![], |result| {
                        result
                            .vertices
                            .into_iter()
                            .filter(|target| target.identity.v_id != self.v_id)
                            .collect()
                    });
                Ok(result)
            }
            Err(err) => {
                let err_message = format!(
                    "TigerGraph neighbors_with_source parse_body error: {:?}",
                    err
                );
                error!(err_message);
                return Err(err);
            }
        }
    }

    /// Return all neighbors of this identity with traversal paths.
    pub async fn neighbors_with_traversal(
        &self,
        client: &Client<HttpConnector>,
        depth: u16,
    ) -> Result<Vec<EdgeUnion>, Error> {
        // query see in Solution: CREATE QUERY neighbors(VERTEX<Identities> p, INT depth)
        let uri: http::Uri = format!(
            "{}/query/{}/neighbors?p={}&depth={}",
            C.tdb.host,
            Graph::IdentityGraph.to_string(),
            self.v_id,
            depth,
        )
        .parse()
        .map_err(|_err: InvalidUri| {
            Error::ParamError(format!(
                "QUERY neighbors_with_traversal({},{}) Uri format Error {}",
                self.v_id, depth, _err
            ))
        })?;
        tracing::trace!("query neighbors_with_traversal Url {:?}", uri);
        let req = hyper::Request::builder()
            .method(Method::GET)
            .uri(uri)
            .header("Authorization", Graph::IdentityGraph.token())
            .body(Body::empty())
            .map_err(|_err| Error::ParamError(format!("ParamError Error {}", _err)))?;
        let mut resp = client.request(req).await.map_err(|err| {
            Error::ManualHttpClientError(format!(
                "query neighbors_with_traversal | Fail to request: {:?}",
                err.to_string()
            ))
        })?;
        match parse_body::<NeighborsResponse>(&mut resp).await {
            Ok(r) => {
                if r.base.error {
                    let err_message = format!(
                        "TigerGraph query neighbors_with_traversal error | Code: {:?}, Message: {:?}",
                        r.base.code, r.base.message
                    );
                    error!(err_message);
                    return Err(Error::General(err_message, resp.status()));
                }

                let result = r
                    .results
                    .and_then(|vec_unions| vec_unions.first().cloned())
                    .map_or(vec![], |union| union.edges);
                Ok(result)
            }
            Err(err) => {
                let err_message = format!(
                    "TigerGraph query neighbors_with_traversal parse_body error: {:?}",
                    err
                );
                error!(err_message);
                return Err(err);
            }
        }
    }

    /// Return from, to by query with source tags.
    pub async fn find_identity_by_source(
        &self,
        client: &Client<HttpConnector>,
        source: &DataSource,
    ) -> Result<Vec<IdentityRecord>, Error> {
        let uri: http::Uri = format!(
            "{}/query/{}/identity_by_source?p={}&source={}",
            C.tdb.host,
            Graph::IdentityGraph.to_string(),
            self.v_id.to_string(),
            source.to_string()
        )
        .parse()
        .map_err(|_err: InvalidUri| Error::ParamError(format!("Uri format Error {}", _err)))?;
        let req = hyper::Request::builder()
            .method(Method::GET)
            .uri(uri)
            .header("Authorization", Graph::IdentityGraph.token())
            .body(Body::empty())
            .map_err(|_err| Error::ParamError(format!("ParamError Error {}", _err)))?;
        let mut resp = client.request(req).await.map_err(|err| {
            Error::ManualHttpClientError(format!(
                "query identity_by_source | Fail to request: {:?}",
                err.to_string()
            ))
        })?;

        match parse_body::<IdentityBySourceResponse>(&mut resp).await {
            Ok(r) => {
                if r.base.error {
                    let err_message = format!(
                        "TigerGraph query identity_by_source error | Code: {:?}, Message: {:?}",
                        r.base.code, r.base.message
                    );
                    error!(err_message);
                    return Err(Error::General(err_message, resp.status()));
                }

                let result = r
                    .results
                    .and_then(|vec_unions| vec_unions.first().cloned())
                    .map_or(vec![], |union| union.vertices);
                Ok(result)
            }
            Err(err) => {
                let err_message = format!(
                    "TigerGraph query identity_by_source parse_body error: {:?}",
                    err
                );
                error!(err_message);
                return Err(err);
            }
        }
    }

    /// Return primary domain names where they would typically only show addresses.
    pub async fn resolve_reverse_domains(
        &self,
        client: &Client<HttpConnector>,
    ) -> Result<Vec<ResolveReverse>, Error> {
        let uri: http::Uri = format!(
            "{}/query/{}/reverse_domains?p={}",
            C.tdb.host,
            Graph::IdentityGraph.to_string(),
            self.v_id.to_string(),
        )
        .parse()
        .map_err(|_err: InvalidUri| Error::ParamError(format!("Uri format Error {}", _err)))?;

        let req = hyper::Request::builder()
            .method(Method::GET)
            .uri(uri)
            .header("Authorization", Graph::IdentityGraph.token())
            .body(Body::empty())
            .map_err(|_err| Error::ParamError(format!("ParamError Error {}", _err)))?;

        let mut resp = client.request(req).await.map_err(|err| {
            Error::ManualHttpClientError(format!(
                "query reverse_domains | Fail to request: {:?}",
                err.to_string()
            ))
        })?;
        match parse_body::<ReverseDomainsResponse>(&mut resp).await {
            Ok(r) => {
                if r.base.error {
                    let err_message = format!(
                        "TigerGraph query reverse_domains error | Code: {:?}, Message: {:?}",
                        r.base.code, r.base.message
                    );
                    error!(err_message);
                    return Err(Error::General(err_message, resp.status()));
                }
                let result: Vec<ResolveReverse> = r
                    .results
                    .and_then(|vec_res| vec_res.first().cloned())
                    .map_or(vec![], |result| {
                        result
                            .reverse_records
                            .into_iter()
                            .map(|record| {
                                let mut resolve_reverse =
                                    ResolveReverse::from(record.attributes.clone());
                                // set 'reverse' to true.
                                resolve_reverse.reverse = true;
                                resolve_reverse
                            })
                            .collect()
                    });
                Ok(result)
            }
            Err(err) => {
                let err_message = format!(
                    "TigerGraph query reverse_domains parse_body error: {:?}",
                    err
                );
                error!(err_message);
                return Err(err);
            }
        }
    }

    /// Return domain-identity owned by another identity: wallet address.
    pub async fn domain_owned_by(
        &self,
        client: &Client<HttpConnector>,
    ) -> Result<Option<IdentityRecord>, Error> {
        // query see in Solution: CREATE QUERY identity_owned_by(VERTEX<Identities> p, STRING platform)
        let uri: http::Uri = format!(
            "{}/query/{}/identity_owned_by?p={}&platform={}",
            C.tdb.host,
            Graph::IdentityGraph.to_string(),
            self.v_id.to_string(),
            self.attributes.platform.to_string(),
        )
        .parse()
        .map_err(|_err: InvalidUri| Error::ParamError(format!("Uri format Error {}", _err)))?;
        let req = hyper::Request::builder()
            .method(Method::GET)
            .uri(uri)
            .header("Authorization", Graph::IdentityGraph.token())
            .body(Body::empty())
            .map_err(|_err| Error::ParamError(format!("ParamError Error {}", _err)))?;
        let mut resp = client.request(req).await.map_err(|err| {
            Error::ManualHttpClientError(format!(
                "query owned_by | Fail to request: {:?}",
                err.to_string()
            ))
        })?;
        match parse_body::<OwnedByResponse>(&mut resp).await {
            Ok(r) => {
                if r.base.error {
                    let err_message = format!(
                        "TigerGraph query owned_by error | Code: {:?}, Message: {:?}",
                        r.base.code, r.base.message
                    );
                    error!(err_message);
                    return Err(Error::General(err_message, resp.status()));
                }
                let result = r
                    .results
                    .and_then(|results| results.first().cloned())
                    .map(|owner| owner.owner)
                    .and_then(|res| res.first().cloned());
                Ok(result)
            }
            Err(err) => {
                let err_message = format!("TigerGraph query owned_by parse_body error: {:?}", err);
                error!(err_message);
                return Err(err);
            }
        }
    }

    /// Returns all Contracts owned by this identity. Empty list if `self.platform != Ethereum`.
    pub async fn nfts(
        &self,
        client: &Client<HttpConnector>,
        category: Option<Vec<ContractCategory>>,
        limit: u16,
        offset: u16,
    ) -> Result<Vec<HoldRecord>, Error> {
        if self.attributes.platform != Platform::Ethereum {
            return Ok(vec![]);
        }
        // query see in Solution: nfts(VERTEX<Identities> p, SET<STRING> categories, INT numPerPage, INT pageNum)
        let uri: http::Uri;
        if category.is_none() || category.as_ref().unwrap().len() == 0 {
            uri = format!(
                "{}/query/{}/nfts?p={}&numPerPage={}&pageNum={}",
                C.tdb.host,
                Graph::IdentityGraph.to_string(),
                self.v_id.to_string(),
                limit,
                offset
            )
            .parse()
            .map_err(|_err: InvalidUri| Error::ParamError(format!("Uri format Error {}", _err)))?;
        } else {
            let categories: Vec<String> = category
                .unwrap()
                .into_iter()
                .map(|field| format!("categories={}", field.to_string()))
                .collect();
            let combined = categories.join("&");
            uri = format!(
                "{}/query/{}/nfts?p={}&{}&numPerPage={}&pageNum={}",
                C.tdb.host,
                Graph::IdentityGraph.to_string(),
                self.v_id.to_string(),
                combined,
                limit,
                offset
            )
            .parse()
            .map_err(|_err: InvalidUri| Error::ParamError(format!("Uri format Error {}", _err)))?;
        }
        let req = hyper::Request::builder()
            .method(Method::GET)
            .uri(uri)
            .header("Authorization", Graph::IdentityGraph.token())
            .body(Body::empty())
            .map_err(|_err| Error::ParamError(format!("ParamError Error {}", _err)))?;
        let mut resp = client.request(req).await.map_err(|err| {
            Error::ManualHttpClientError(format!(
                "query nfts | Fail to request: {:?}",
                err.to_string()
            ))
        })?;
        match parse_body::<QueryNftsResponse>(&mut resp).await {
            Ok(r) => {
                if r.base.error {
                    let err_message = format!(
                        "TigerGraph query nfts error | Code: {:?}, Message: {:?}",
                        r.base.code, r.base.message
                    );
                    error!(err_message);
                    return Err(Error::General(err_message, resp.status()));
                }

                let result = r
                    .results
                    .and_then(|vec_unions| vec_unions.first().cloned())
                    .map_or(vec![], |union| union.edges);
                Ok(result)
            }
            Err(err) => {
                let err_message = format!("TigerGraph query nfts parse_body error: {:?}", err);
                error!(err_message);
                return Err(err);
            }
        }
    }
}

pub struct IdentityLoadFn {
    pub client: Client<HttpConnector>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct VertexIds {
    ids: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct VertexIdsResponse {
    #[serde(flatten)]
    base: BaseResponse,
    results: Option<Vec<VertexIdsResult>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct VertexIdsResult {
    vertices: Vec<IdentityRecord>,
}

#[async_trait::async_trait]
impl BatchFn<String, Option<IdentityRecord>> for IdentityLoadFn {
    async fn load(&mut self, ids: &[String]) -> HashMap<String, Option<IdentityRecord>> {
        trace!(ids = ids.len(), "Loading Identity id");
        let records = get_identities_by_ids(&self.client, ids.to_vec()).await;
        match records {
            Ok(records) => records,
            // HOLD ON: Not sure if `Err` need to return
            Err(_) => ids.iter().map(|k| (k.to_owned(), None)).collect(),
        }
    }
}

async fn get_identities_by_ids(
    client: &Client<HttpConnector>,
    ids: Vec<String>,
) -> Result<HashMap<String, Option<IdentityRecord>>, Error> {
    let uri: http::Uri = format!(
        "{}/query/{}/identities_by_ids",
        C.tdb.host,
        Graph::IdentityGraph.to_string()
    )
    .parse()
    .map_err(|_err: InvalidUri| Error::ParamError(format!("Uri format Error {}", _err)))?;
    let payload = VertexIds { ids };
    let json_params = serde_json::to_string(&payload).map_err(|err| Error::JSONParseError(err))?;
    let req = hyper::Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header("Authorization", Graph::IdentityGraph.token())
        .body(Body::from(json_params))
        .map_err(|_err| Error::ParamError(format!("ParamError Error {}", _err)))?;
    let mut resp = client.request(req).await.map_err(|err| {
        Error::ManualHttpClientError(format!(
            "TigerGraph | Fail to request identities_by_ids: {:?}",
            err.to_string()
        ))
    })?;
    match parse_body::<VertexIdsResponse>(&mut resp).await {
        Ok(r) => {
            if r.base.error {
                let err_message = format!(
                    "TigerGraph identities_by_ids error | Code: {:?}, Message: {:?}",
                    r.base.code, r.base.message
                );
                error!(err_message);
                return Err(Error::General(err_message, resp.status()));
            }

            let result = r
                .results
                .and_then(|results| results.first().cloned())
                .map_or(vec![], |res| res.vertices)
                .into_iter()
                .map(|content| (content.v_id.clone(), Some(content)))
                .collect();
            Ok(result)
        }
        Err(err) => {
            let err_message = format!("TigerGraph identities_by_ids parse_body error: {:?}", err);
            error!(err_message);
            return Err(err);
        }
    }
}
