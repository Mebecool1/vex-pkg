use std::fs;

pub fn fetch_pkg(url: &str, pkg_name: &str) {
    let endpoint = format!("{}/vex/pkgs/{}.tar", url.trim_end_matches('/'), pkg_name);
    let response = ureq::get(&endpoint)
        .call()
        .expect("Failed to reach vex server");

    let mut file = fs::File::create(format!("{}_pkg.tar", pkg_name)).expect("Failed to create tarball");

    let data = response
        .into_body()
        .read_to_vec()
        .expect("Failed to read tarball");
    std::io::Write::write_all(&mut file, &data).expect("Failed to write tarball");

    println!("fetched {} -> {}_pkg.tar", pkg_name, pkg_name);
}

pub fn list_pkgs_from_url(url: &str) -> Vec<String> {
    let endpoint = format!("{}/vex/pkgs/", url.trim_end_matches('/'));
    let response = ureq::get(&endpoint)
        .call()
        .expect("Failed to reach vex server");

    let body = response
        .into_body()
        .read_to_string()
        .expect("Failed to read response");

    body.lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect()
}
pub fn fetch_pkg_to(url: &str, pkg_name: &str, dest: &str) {
    let endpoint = format!("{}/vex/pkgs/{}.tar", url.trim_end_matches('/'), pkg_name);
    let response = ureq::get(&endpoint).call().expect("Failed to reach vex server");
    let mut file = fs::File::create(dest).expect("Failed to create tarball");
    let mut body = response.into_body();
    std::io::copy(&mut body.as_reader(), &mut file).expect("Failed to write tarball");
    println!("fetched {} -> {}", pkg_name, dest);
}