use std::fs;
use tiny_http::{Response, Server};

pub fn serve(port: u16) {
    let server = Server::http(format!("127.0.0.1:{port}")).unwrap();
    println!("vex serving on http://127.0.0.1:{port}/vex/pkgs/");

    for request in server.incoming_requests() {
        let url = request.url().to_string();
        println!("DEBUG: got request for {}", url);
        if url == "/vex/pkgs/" {
            // listing
            let tarballs: Vec<String> = fs::read_dir(".")
                .unwrap()
                .filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|name| name.ends_with(".tar"))
                .collect();
            request
                .respond(Response::from_string(tarballs.join("\n")))
                .unwrap();
        } else if url.starts_with("/vex/pkgs/") {
            // serve specific package
            let filename = url.trim_start_matches("/vex/pkgs/");
            match fs::read(filename) {
                Ok(data) => request.respond(Response::from_data(data)).unwrap(),
                Err(_) => request.respond(Response::from_string("not found")).unwrap(),
            }
        } else {
            request
                .respond(Response::from_string("vex server"))
                .unwrap();
        }
    }
}

