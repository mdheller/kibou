pub mod activity;
pub mod actor;
pub mod controller;
pub mod routes;
pub mod validator;

use base64;
use rocket::data::{self, Data, FromDataSimple};
use rocket::http::ContentType;
use rocket::http::MediaType;
use rocket::http::Status;
use rocket::request::{self, FromRequest, Request};
use rocket::response::{self, Responder, Response};
use rocket::Outcome;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Cursor, Read};
use web::http_signatures::Signature;

pub struct ActivitypubMediatype(bool);
pub struct ActivitystreamsResponse(String);

// ActivityStreams2/AcitivityPub properties are expressed in CamelCase
#[allow(non_snake_case)]
#[derive(Serialize, Deserialize)]
pub struct Attachment {
    #[serde(rename = "type")]
    pub _type: String,
    pub content: Option<String>,
    pub url: String,
    pub name: Option<String>,
    pub mediaType: Option<String>,
}

pub struct Payload(serde_json::Value);

impl FromDataSimple for Payload {
    type Error = String;

    fn from_data(req: &Request, data: Data) -> data::Outcome<Self, String> {
        let mut data_stream = String::new();

        // Read at most a 1MB payload
        //
        // TODO: This value should be adjustable in the config
        if let Err(e) = data.open().take(1048576).read_to_string(&mut data_stream) {
            return Outcome::Failure((Status::InternalServerError, format!("{:?}", e)));
        }

        match serde_json::from_str(&data_stream) {
            Ok(value) => return Outcome::Success(Payload(value)),
            Err(e) => return Outcome::Failure((Status::UnprocessableEntity, format!("{:?}", e))),
        }
    }
}

impl<'a, 'r> FromRequest<'a, 'r> for ActivitypubMediatype {
    type Error = ();

    fn from_request(request: &'a Request<'r>) -> request::Outcome<ActivitypubMediatype, ()> {
        let activitypub_default = MediaType::with_params(
            "application",
            "ld+json",
            ("profile", "https://www.w3.org/ns/activitystreams"),
        );
        let activitypub_lite = MediaType::new("application", "activity+json");

        match request.accept() {
            Some(accept) => {
                if accept
                    .media_types()
                    .find(|t| t == &&activitypub_default)
                    .is_some()
                    || accept
                        .media_types()
                        .find(|t| t == &&activitypub_lite)
                        .is_some()
                {
                    Outcome::Success(ActivitypubMediatype(true))
                } else {
                    Outcome::Forward(())
                }
            }
            None => Outcome::Forward(()),
        }
    }
}

impl<'r> Responder<'r> for ActivitystreamsResponse {
    fn respond_to(self, _: &Request) -> response::Result<'r> {
        Response::build()
            .header(ContentType::new("application", "activity+json"))
            .sized_body(Cursor::new(self.0))
            .ok()
    }
}

impl<'a, 'r> FromRequest<'a, 'r> for Signature {
    type Error = ();

    fn from_request(request: &'a Request<'r>) -> request::Outcome<Signature, ()> {
        let content_length_vec: Vec<_> = request.headers().get("Content-Length").collect();
        let date_vec: Vec<_> = request.headers().get("Date").collect();
        let digest_vec: Vec<_> = request.headers().get("Digest").collect();
        let host_vec: Vec<_> = request.headers().get("Host").collect();
        let signature_vec: Vec<_> = request.headers().get("Signature").collect();

        if signature_vec.is_empty() {
            return Outcome::Failure((Status::BadRequest, ()));
        } else {
            let parsed_signature: HashMap<String, String> = signature_vec[0]
                .replace("\"", "")
                .to_string()
                .split(',')
                .map(|kv| kv.split('='))
                .map(|mut kv| (kv.next().unwrap().into(), kv.next().unwrap().into()))
                .collect();

            let headers: Vec<&str> = parsed_signature["headers"].split_whitespace().collect();
            let route = request.route().unwrap().to_string();
            let request_target: Vec<&str> = route.split_whitespace().collect();

            return Outcome::Success(Signature {
                algorithm: None,
                content_length: Some(content_length_vec.get(0).unwrap_or_else(|| &"").to_string()),
                date: date_vec.get(0).unwrap_or_else(|| &"").to_string(),
                digest: Some(digest_vec.get(0).unwrap_or_else(|| &"").to_string()),
                headers: headers.iter().map(|header| header.to_string()).collect(),
                host: host_vec.get(0).unwrap_or_else(|| &"").to_string(),
                key_id: None,
                request_target: Some(request_target[1].to_string()),
                signature: String::new(),
                signature_in_bytes: Some(
                    base64::decode(&parsed_signature["signature"].to_owned().into_bytes()).unwrap(),
                ),
            });
        }
    }
}
