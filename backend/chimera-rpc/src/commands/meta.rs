//! Caller-scoped discovery, copied from the upstream meta command.
use crate::registry::RpcDescribeContext;
use chimera_rpc_protocol::RpcDescribeResponse;

#[chimera_rpc::rpc(query, operation = chimera_rpc_protocol::operations::RpcDescribe, expose = [http, local])]
pub fn rpc_describe(context: RpcDescribeContext) -> RpcDescribeResponse {
    context.into_response()
}
