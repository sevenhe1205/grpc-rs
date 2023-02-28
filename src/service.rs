
pub use self::echo::{echo_client, echo_server};
pub use self::echo::echo_server::{Echo, EchoServer};

use tonic::Code;
use echo::{EchoRequest, EchoResponse};
use tonic::{Request, Response, Status};
use futures::FutureExt;

#[allow(unused)]
use tracing::{trace, debug, info, warn, error};

pub(crate) const GRPC_REQUESTS_COUNTER: &str = "grpc_requests_total";
#[derive(Debug)]
pub struct EchoService {
}

impl EchoService {
    pub fn new() -> Self {
        EchoService {
        }
    }
}

pub mod echo {
    tonic::include_proto!("grpc.echo.v1");
    pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("grpc_descriptor");
}

#[tonic::async_trait]
impl Echo for EchoService {

    async fn unary_echo(&self, request: Request<EchoRequest>) -> Result<Response<EchoResponse>, Status> {
        let req = request.into_inner();
        process_request(req)
            .inspect(|rst| {
                metrics::increment_counter!(GRPC_REQUESTS_COUNTER,
                    "api" => "handle",
                    "status" => rst.as_ref().map_or("error", |_| "ok"),
                );
            })
            .await
            .map(Response::new)
    }
}

async fn process_request<T, E>(req: T) -> Result<EchoResponse, Status>
where
    T: TryInto<EchoRequest, Error = E>,
    E: std::fmt::Debug + std::fmt::Display + Send + Sync + 'static,
{
    let req: EchoRequest = req.try_into().map_err(|e| {
        trace!(error = ?e, "Failed to convert to ServiceRequest");
        Status::new(Code::InvalidArgument, "Failed to convert to ServiceRequest")
    })?;

    debug!("Processing gRPC request");

    Ok(EchoResponse {
        message: req.message.into(),
    })
}
