use std::sync::Arc;

use anyhow::bail;
use http::header::HOST;
use http::uri::Scheme;
use http::{HeaderValue, Uri};
use tokio::sync::oneshot;
use wasmtime::component::{Accessor, AccessorTask, HostFuture, Resource};
use wasmtime_wasi::p3::{ResourceView as _, WithChildren};

use crate::p3::bindings::http::handler;
use crate::p3::bindings::http::types::ErrorCode;
use crate::p3::{Body, Request, Response, WasiHttpImpl, WasiHttpView};

use super::{delete_request, get_fields_inner};

struct TrailerTask {
    rx: HostFuture<Result<Option<Resource<WithChildren<http::HeaderMap>>>, ErrorCode>>,
    tx: oneshot::Sender<Result<Option<http::HeaderMap>, ErrorCode>>,
}

impl<T, U: WasiHttpView> AccessorTask<T, U, wasmtime::Result<()>> for TrailerTask {
    async fn run(self, store: &mut Accessor<T, U>) -> wasmtime::Result<()> {
        let rx = store.with(|mut view| self.rx.into_reader(&mut view));
        match rx.read().into_future().await {
            Ok(Ok(trailers)) => store.with(|mut view| {
                let trailers = trailers
                    .map(|trailers| get_fields_inner(view.table(), &trailers))
                    .transpose()?;
                _ = self.tx.send(Ok(trailers.as_deref().cloned()));
                Ok(())
            }),
            Ok(Err(err)) => {
                _ = self.tx.send(Err(err));
                Ok(())
            }
            Err(..) => Ok(()),
        }
    }
}

impl<T> handler::Host for WasiHttpImpl<&mut T>
where
    T: WasiHttpView + 'static,
{
    async fn handle<U: 'static>(
        store: &mut Accessor<U, Self>,
        request: Resource<Request>,
    ) -> wasmtime::Result<Result<Resource<Response>, ErrorCode>> {
        let Request {
            method,
            scheme,
            authority,
            path_with_query,
            headers,
            mut body,
            options,
            ..
        } = store.with(|mut view| delete_request(view.table(), request))?;

        // TODO: Validate in `send_request`
        //let (scheme, use_tls) = match scheme {
        //    None => (Scheme::HTTPS, true),
        //    Some(scheme) if scheme == Scheme::HTTP => (Scheme::HTTP, false),
        //    Some(scheme) if scheme == Scheme::HTTPS => (Scheme::HTTPS, true),
        //    // We can only support HTTP/HTTPS
        //    Some(..) => return Ok(Err(ErrorCode::HttpProtocolError)),
        //};

        let options = options
            .map(|options| options.unwrap_or_clone())
            .transpose()?;
        let headers = headers.unwrap_or_clone()?;
        // TODO: if `set_host_header`
        //let host = if let Some(authority) = authority.as_ref() {
        //    match HeaderValue::try_from(authority.as_str()) {
        //        Ok(host) => host,
        //        Err(err) => return Ok(Err(ErrorCode::InternalError(Some(err.to_string())))),
        //    }
        //} else {
        //    HeaderValue::from_static("")
        //};
        //headers.insert(HOST, host);
        let mut uri = Uri::builder();
        if let Some(scheme) = scheme {
            uri = uri.scheme(scheme)
        };
        if let Some(authority) = authority {
            uri = uri.authority(authority)
        };
        if let Some(path_with_query) = path_with_query {
            uri = uri.path_and_query(path_with_query)
        };
        let Ok(uri) = uri.build() else {
            return Ok(Err(ErrorCode::HttpRequestUriInvalid));
        };

        let Some(body) = Arc::get_mut(&mut body) else {
            return Ok(Err(ErrorCode::InternalError(Some(
                "body is borrowed".into(),
            ))));
        };
        let Ok(body) = body.get_mut() else {
            bail!("lock poisoned");
        };
        if matches!(body, Body::Consumed) {
            return Ok(Err(ErrorCode::InternalError(Some(
                "body is consumed".into(),
            ))));
        }

        let mut request = http::Request::builder();
        *request.headers_mut().unwrap() = headers;
        request = request.method(method).uri(uri);
        // TODO
        todo!()
        //let response = match body {
        //    Body::Outgoing {
        //        contents: stream,
        //        trailers: finish,
        //        tx,
        //    } => {
        //        let (trailers_tx, trailers_rx) = oneshot::channel();
        //        let task = store.spawn(TrailerTask {
        //            rx: finish,
        //            tx: trailers_tx,
        //        });
        //        let stream = store
        //            .with(|mut view| stream.into_reader(&mut view))
        //            .read()
        //            .into_future();
        //        let request = match request.body(GuestBody {
        //            stream: Some(stream),
        //            trailers: Some(trailers_rx),
        //            trailer_task: task.abort_handle(),
        //        }) {
        //            Ok(request) => request,
        //            Err(err) => return Ok(Err(ErrorCode::InternalError(Some(err.to_string())))),
        //        };
        //        let fut = store.with(|mut view| view.send_request(request, options));
        //        match fut.await? {
        //            Ok(response) => response,
        //            Err(err) => return Ok(Err(err)),
        //        }
        //    }
        //    // TODO
        //    _ => todo!(),
        //    //Body::OutgoingStream { body, tx } => {
        //    //    bail!("TODO")
        //    //    //let Some(body) = body else {
        //    //    //    return Ok(Err(ErrorCode::InternalError(Some(
        //    //    //        "body is finished".into(),
        //    //    //    ))));
        //    //    //};
        //    //    //(body, Some(tx))
        //    //}
        //    //Body::Child(..) => bail!("TODO"),
        //    //Body::Incoming => bail!("TODO"),
        //    //Body::Corrupted => bail!("body is corrupted"),
        //};
        //// TODO: call send_request(request)

        ////if let Some(tx) = tx {
        ////    tx.
        ////}

        ////let authority = if authority.port().is_some() {
        ////    authority.to_string()
        ////} else {
        ////    let port = if use_tls { 443 } else { 80 };
        ////    format!("{authority}:{port}")
        ////};

        ////let connect_timeout = options
        ////    .and_then(
        ////        |RequestOptions {
        ////             connect_timeout, ..
        ////         }| connect_timeout.map(Duration::from_nanos),
        ////    )
        ////    .unwrap_or(Duration::from_secs(600));

        ////let first_byte_timeout = options
        ////    .and_then(
        ////        |RequestOptions {
        ////             first_byte_timeout, ..
        ////         }| first_byte_timeout.map(Duration::from_nanos),
        ////    )
        ////    .unwrap_or(Duration::from_secs(600));

        //// TODO
        //Ok(Err(ErrorCode::InternalError(None)))
        ////})
    }
}
