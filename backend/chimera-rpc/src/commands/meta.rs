//! Capability discovery: `rpc_describe`, which every resolved caller may call.
//!
//! `meta` holds this one command; every resolved caller holds `meta:read`.

chimera_rpc::namespace!(expose = [http, local]);

use crate::registry::RpcDescribeContext;
use chimera_rpc_protocol::RpcDescribeResponse;

#[chimera_rpc::rpc(query, operation = chimera_rpc_protocol::operations::RpcDescribe, expose = [http, local])]
pub fn rpc_describe(context: RpcDescribeContext) -> RpcDescribeResponse {
    context.into_response()
}
