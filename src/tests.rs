use std::fs;

use super::rocket;
use rocket::http::{Header, Status};
use rocket::local::blocking::Client;

#[test]
fn api() {
    let client = Client::tracked(rocket()).expect("valid rocket instance");
    let response = client.get("/api").dispatch();
    assert_eq!(response.status(), Status::Ok);
    assert_eq!(
        response.into_string().unwrap(),
        format!(
            "{{\"version\":\"{}\"}}",
            env!("CARGO_PKG_VERSION").to_string()
        )
    );
}

#[test]
fn index() {
    let client = Client::tracked(rocket()).expect("valid rocket instance");
    let response = client.get("/").dispatch();
    assert_ne!(response.status(), Status::InternalServerError)
}

#[test]
fn upload() {
    let _ = fs::create_dir("files/uploads/");
    let client = Client::tracked(rocket()).expect("valid rocket instance");

    let data = "--TEST-BOUNDARY\r\n\
Content-Disposition: form-data; name=\"files\"; filename=\"upload.txt\"\r\n\
Content-Type: text/plain\r\n\r\n\
MARMAK Mirror testing!\r\n\
--TEST-BOUNDARY\r\n\
Content-Disposition: form-data; name=\"path\"\r\n\r\n\
/uploads/\r\n\
--TEST-BOUNDARY--\r\n\
";

    let response = client
        .post("/api/upload")
        .header(Header::new(
            "Content-Type",
            "multipart/form-data; boundary=TEST-BOUNDARY",
        ))
        .body(data)
        .dispatch();

    assert_eq!(response.status(), Status::Ok);

    let _ = fs::remove_file("files/uploads/upload.txt");
}

#[test]
fn rename() {
    let _ = fs::File::create("files/rename.txt").expect("Failed to create file");
    let client = Client::tracked(rocket()).expect("valid rocket instance");
    let data = "{\"name\":\"file.txt\"}";
    let response = client.patch("/api/rename.txt").body(data).dispatch();

    assert_eq!(response.status(), Status::Ok);

    let _ = fs::remove_file("files/file.txt");
}

#[test]
fn delete() {
    let _ = fs::File::create("files/delete.txt").expect("Failed to create file");

    let client = Client::tracked(rocket()).expect("valid rocket instance");
    let response = client.delete("/api/delete.txt").dispatch();

    assert_eq!(response.status(), Status::NoContent);
}

#[test]
fn strings() {
    let languages: Vec<(String, String)> = crate::TranslationStore::new().available_languages().to_vec();
    let language_codes: Vec<&str> = languages.iter().map(|l| l.0.as_str()).collect();

    for lang in language_codes {
        let client = Client::tracked(rocket()).expect("valid rocket instance");
        let response = client.get(format!("/test/strings?lang={}", lang)).dispatch();
        assert_eq!(response.status(), Status::Ok)
    }
}
