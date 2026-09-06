use crate::print;
use std::fs;
use tiny_http::{Response, Server};

pub fn serve(port: u16, ip: &str) {
    let server = Server::http(format!("{ip}:{port}")).unwrap();
    print::vex_print("Serving", &format!("http://{ip}:{port}/vex/pkgs/"));
    for request in server.incoming_requests() {
        let url = request.url().to_string();
        if url == "/vex/pkgs/" {
            let tarballs: Vec<String> = fs::read_dir(".")
                .unwrap()
                .filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|name| name.ends_with(".tar.zst"))
                .collect();
            print::vex_print("List", &format!("{} package(s)", tarballs.len()));
            request
                .respond(Response::from_string(tarballs.join("\n")))
                .unwrap();
        } else if url.starts_with("/vex/pkgs/") {
            let filename = url.trim_start_matches("/vex/pkgs/");
            match fs::read(filename) {
                Ok(data) => {
                    print::vex_print("Serve", filename);
                    request.respond(Response::from_data(data)).unwrap();
                }
                Err(_) => {
                    print::vex_warn(&format!("not found: {}", filename));
                    request.respond(Response::from_string("not found")).unwrap();
                }
            }
        } else {
            request
                .respond(Response::from_string("vex server"))
                .unwrap();
        }
    }
}
