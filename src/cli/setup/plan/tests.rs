use crate::{
    cli::setup::plan::*,
    config::{Config, TEMPLATE},
};

#[test]
fn first_package_manager_found_is_used() {
    assert_eq!(
        PackageManager::detect(|program| program == "dnf"),
        Some(PackageManager::Dnf)
    );
    assert_eq!(
        PackageManager::detect(|program| program == "apt-get" || program == "dnf"),
        Some(PackageManager::Apt)
    );
    assert_eq!(PackageManager::detect(|_| false), None);
}

#[test]
fn apt_refreshes_its_lists_before_installing() {
    assert_eq!(
        PackageManager::Apt.install_commands(&[Package::Ffmpeg, Package::Python]),
        [
            vec!["apt-get", "update", "-q"],
            vec!["apt-get", "install", "-y", "-q", "ffmpeg", "python3"],
        ]
    );
}

#[test]
fn package_names_follow_the_distribution() {
    assert_eq!(
        PackageManager::Dnf.install_commands(&[Package::Ffmpeg]),
        [vec!["dnf", "install", "-y", "ffmpeg-free"]]
    );
    assert_eq!(
        PackageManager::Pacman.install_commands(&[Package::Python, Package::Curl]),
        [vec![
            "pacman",
            "-S",
            "--needed",
            "--noconfirm",
            "python",
            "curl"
        ]]
    );
}

#[test]
fn packages_are_listed_once_and_only_when_missing() {
    let nothing = |_: &str| false;
    assert_eq!(
        packages(&[Tool::YtDlp, Tool::Deno], nothing),
        [Package::Python, Package::Curl]
    );
    let has_curl = |program: &str| program == "curl";
    assert_eq!(packages(&[Tool::Deno], has_curl), [Package::Python]);
    let everything = |_: &str| true;
    assert_eq!(packages(&[Tool::YtDlp], everything), []);
}

#[test]
fn chosen_ffmpeg_is_installed_even_when_present() {
    assert_eq!(packages(&[Tool::Ffmpeg], |_| true), [Package::Ffmpeg]);
}

#[test]
fn missing_tools_are_preselected() {
    let installed = [
        (Tool::Ffmpeg, Some("7.1".to_owned())),
        (Tool::YtDlp, None),
        (Tool::Deno, None),
    ];
    assert_eq!(preselected(&installed, false), [Tool::YtDlp, Tool::Deno]);
    assert_eq!(preselected(&installed, true), Tool::ALL);
}

#[test]
fn deno_has_builds_for_two_architectures() {
    let download = Tool::Deno.download("x86_64").unwrap();
    assert_eq!(download.file, "deno-x86_64-unknown-linux-gnu.zip");
    assert!(download.zipped);
    assert_eq!(
        download.sums_url,
        "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-unknown-linux-gnu.zip.sha256sum"
    );
    assert!(Tool::Deno.download("aarch64").is_some());
    assert!(Tool::Deno.download("riscv64").is_none());
    assert!(Tool::Ffmpeg.download("x86_64").is_none());
}

#[test]
fn yt_dlp_is_the_zipapp() {
    let download = Tool::YtDlp.download("aarch64").unwrap();
    assert_eq!(download.file, "yt-dlp");
    assert!(download.url.ends_with("/releases/latest/download/yt-dlp"));
}

#[test]
fn hash_is_found_by_the_exact_file_name() {
    let sums = "aaa  yt-dlp\nbbb  yt-dlp.exe\nccc *yt-dlp_linux\n";
    assert_eq!(expected_hash(sums, "yt-dlp"), Some("aaa"));
    assert_eq!(expected_hash(sums, "yt-dlp_linux"), Some("ccc"));
    assert_eq!(expected_hash(sums, "yt-dlp.tar.gz"), None);
    assert_eq!(expected_hash("", "yt-dlp"), None);
}

#[test]
fn config_keeps_the_state_in_var_lib() {
    let text = config_file(TEMPLATE);
    let config: Config = toml::from_str(&text).unwrap();
    assert_eq!(config.paths.state_dir, std::path::Path::new(STATE_DIR));
    assert_eq!(
        config.paths.secrets_file,
        std::path::Path::new("secrets.toml")
    );
}

#[test]
fn unit_runs_the_bot_as_its_own_user() {
    let unit = unit_file();
    assert!(unit.contains("\nUser=tsmusic\n"));
    assert!(
        unit.contains("\nExecStart=/usr/local/bin/tsmusic --config /etc/tsmusic/config.toml run\n")
    );
    assert!(unit.contains("\nRestart=on-failure\n"));
    assert!(unit.contains("\nStateDirectory=tsmusic\n"));
    assert!(unit.contains("\nWantedBy=multi-user.target\n"));
}
