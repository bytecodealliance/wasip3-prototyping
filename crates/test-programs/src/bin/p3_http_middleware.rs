use {
    flate2::{
        Compression,
        write::{DeflateDecoder, DeflateEncoder},
    },
    std::{io::Write, mem},
    test_programs::p3::{
        proxy::exports::wasi::http::handler::Guest as Handler,
        wasi::http::{
            handler,
            types::{ErrorCode, Headers, Request, Response},
        },
        wit_future, wit_stream,
    },
    wit_bindgen_rt::async_support::{self, StreamResult},
};

struct Component;

test_programs::p3::proxy::export!(Component);

impl Handler for Component {
    /// Forward the specified request to the imported `wasi:http/handler`, transparently decoding the request body
    /// if it is `deflate`d and then encoding the response body if the client has provided an `accept-encoding:
    /// deflate` header.
    async fn handle(request: Request) -> Result<Response, ErrorCode> {
        // First, extract the parts of the request and check for (and remove) headers pertaining to body encodings.
        let method = request.method();
        let scheme = request.scheme();
        let path_with_query = request.path_with_query();
        let authority = request.authority();
        let mut accept_deflated = false;
        let mut content_deflated = false;
        let headers = request.headers();
        let mut headers = headers.entries();
        headers.retain(|(k, v)| match (k.as_str(), v.as_slice()) {
            ("accept-encoding", value)
                if std::str::from_utf8(value)
                    .map(|v| v.contains("deflate"))
                    .unwrap_or(false) =>
            {
                accept_deflated = true;
                false
            }
            ("content-encoding", b"deflate") => {
                content_deflated = true;
                false
            }
            _ => true,
        });
        let (mut body, trailers) = request.body().unwrap();

        let (body, trailers) = if content_deflated {
            // Next, spawn a task to pipe and decode the original request body and trailers into a new request
            // we'll create below.  This will run concurrently with any code in the imported `wasi:http/handler`.
            let (trailers_tx, trailers_rx) = wit_future::new(|| todo!());
            let (mut pipe_tx, pipe_rx) = wit_stream::new();

            async_support::spawn(async move {
                {
                    let mut decoder = DeflateDecoder::new(Vec::new());

                    let (mut status, mut chunk) = body.read(Vec::with_capacity(64 * 1024)).await;
                    while let StreamResult::Complete(_) = status {
                        decoder.write_all(&chunk).unwrap();
                        let remaining = pipe_tx.write_all(mem::take(decoder.get_mut())).await;
                        assert!(remaining.is_empty());
                        *decoder.get_mut() = remaining;
                        chunk.clear();
                        (status, chunk) = body.read(chunk).await;
                    }

                    let remaining = pipe_tx.write_all(decoder.finish().unwrap()).await;
                    assert!(remaining.is_empty());

                    drop(pipe_tx);
                }

                trailers_tx.write(trailers.await).await.unwrap();

                drop(request);
            });

            (pipe_rx, trailers_rx)
        } else {
            (body, trailers)
        };

        // While the above task (if any) is running, synthesize a request from the parts collected above and pass
        // it to the imported `wasi:http/handler`.
        let (my_request, _request_complete) = Request::new(
            Headers::from_list(&headers).unwrap(),
            Some(body),
            trailers,
            None,
        );
        my_request.set_method(&method).unwrap();
        my_request.set_scheme(scheme.as_ref()).unwrap();
        my_request
            .set_path_with_query(path_with_query.as_deref())
            .unwrap();
        my_request.set_authority(authority.as_deref()).unwrap();

        let response = handler::handle(my_request).await?;

        // Now that we have the response, extract the parts, adding an extra header if we'll be encoding the body.
        let status_code = response.status_code();
        let mut headers = response.headers().entries();
        if accept_deflated {
            headers.push(("content-encoding".into(), b"deflate".into()));
        }

        let (mut body, trailers) = response.body().unwrap();
        let (body, trailers) = if accept_deflated {
            headers.retain(|(name, _value)| name != "content-length");

            // Spawn another task; this one is to pipe and encode the original response body and trailers into a
            // new response we'll create below.  This will run concurrently with the caller's code (i.e. it won't
            // necessarily complete before we return a value).
            let (trailers_tx, trailers_rx) = wit_future::new(|| todo!());
            let (mut pipe_tx, pipe_rx) = wit_stream::new();

            async_support::spawn(async move {
                {
                    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::fast());
                    let (mut status, mut chunk) = body.read(Vec::with_capacity(64 * 1024)).await;

                    while let StreamResult::Complete(_) = status {
                        encoder.write_all(&chunk).unwrap();
                        let remaining = pipe_tx.write_all(mem::take(encoder.get_mut())).await;
                        assert!(remaining.is_empty());
                        *encoder.get_mut() = remaining;
                        chunk.clear();
                        (status, chunk) = body.read(chunk).await;
                    }

                    let remaining = pipe_tx.write_all(encoder.finish().unwrap()).await;
                    assert!(remaining.is_empty());

                    drop(pipe_tx);
                }

                trailers_tx.write(trailers.await).await.unwrap();
                drop(response);
            });

            (pipe_rx, trailers_rx)
        } else {
            (body, trailers)
        };

        // While the above tasks (if any) are running, synthesize a response from the parts collected above and
        // return it.
        let (my_response, _response_complete) =
            Response::new(Headers::from_list(&headers).unwrap(), Some(body), trailers);
        my_response.set_status_code(status_code).unwrap();

        Ok(my_response)
    }
}

// Unused function; required since this file is built as a `bin`:
fn main() {}
