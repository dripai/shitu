use super::*;
use std::io::Cursor;

fn response(version: &str) -> serde_json::Value {
    let name = format!("ShiTu-v{version}-windows-x86_64.zip");
    serde_json::json!({
        "tag_name": format!("v{version}"), "draft": false, "prerelease": false,
        "body": "Release notes", "assets": [
            {"name":name,"size":100,"browser_download_url":format!("{RELEASES}/download/v{version}/{name}")},
            {"name":format!("{name}.sha256"),"size":100,"browser_download_url":format!("{RELEASES}/download/v{version}/{name}.sha256")}
        ]
    })
}
#[test]
fn stable_versions_compare_numerically_and_never_downgrade() -> Result<()> {
    for (remote, current, newer) in [
        ("0.3.0", "0.4.0", false),
        ("0.4.0", "0.4.0", false),
        ("0.10.0", "0.9.0", true),
        ("1.0.0", "0.4.0", true),
    ] {
        let parsed = parse_release(&serde_json::to_vec(&response(remote))?, current)?;
        assert_eq!(parsed.update.is_some(), newer);
    }
    let mut prerelease = response("1.0.0");
    prerelease["prerelease"] = true.into();
    assert!(parse_release(&serde_json::to_vec(&prerelease)?, "0.4.0").is_err());
    assert!(parse_release(&serde_json::to_vec(&response("1.0.0-rc.1"))?, "0.4.0").is_err());
    Ok(())
}
#[test]
fn new_releases_require_matching_unique_bounded_project_assets() -> Result<()> {
    for change in ["missing", "url", "duplicate", "size"] {
        let mut value = response("1.0.0");
        match change {
            "missing" => {
                value["assets"].as_array_mut().unwrap().pop();
            }
            "url" => {
                value["assets"][0]["browser_download_url"] = "https://example.com/evil.zip".into()
            }
            "duplicate" => {
                let first = value["assets"][0].clone();
                value["assets"].as_array_mut().unwrap().push(first);
            }
            _ => value["assets"][0]["size"] = (MAX_ARCHIVE + 1).into(),
        }
        assert!(
            parse_release(&serde_json::to_vec(&value)?, "0.4.0").is_err(),
            "{change}"
        );
    }
    Ok(())
}
#[test]
fn checksum_binds_hash_to_archive_and_responses_are_bounded() -> Result<()> {
    let hash = "a".repeat(64);
    assert_eq!(
        checksum(
            format!("{} *package.zip", hash.to_uppercase()).as_bytes(),
            "package.zip"
        )?,
        hash
    );
    for text in [
        format!("{hash} *other.zip"),
        "abcd *package.zip".into(),
        format!("{hash} *package.zip\n{hash} *package.zip"),
    ] {
        assert!(checksum(text.as_bytes(), "package.zip").is_err());
    }
    assert!(bounded_bytes(Cursor::new(vec![0; 11]), 10).is_err());
    assert_eq!(bounded_bytes(Cursor::new(vec![0; 10]), 10)?.len(), 10);
    Ok(())
}
fn zip_fixture(path: &Path, names: &[&str]) -> Result<()> {
    let mut zip = zip::ZipWriter::new(File::create_new(path)?);
    for name in names {
        zip.start_file(
            *name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored),
        )?;
        zip.write_all(b"fixture contents")?;
    }
    zip.finish()?;
    Ok(())
}
#[test]
fn extraction_rejects_extra_or_traversing_entries_and_never_overwrites() -> Result<()> {
    let root = std::env::temp_dir().join(format!(
        "shitu-update-zip-{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    fs::create_dir(&root)?;
    for (index, names) in [
        vec!["../ShiTu.exe"],
        vec!["ShiTu.exe", "script.cmd"],
        vec!["folder/ShiTu.exe"],
    ]
    .iter()
    .enumerate()
    {
        let archive = root.join(format!("{index}.zip"));
        zip_fixture(&archive, names)?;
        assert!(extract(&archive, &root.join("new.exe")).is_err());
        assert!(!root.join("new.exe").exists());
        fs::remove_file(archive)?;
    }
    let archive = root.join("valid.zip");
    let target = root.join("new.exe");
    zip_fixture(&archive, &["ShiTu.exe"])?;
    extract(&archive, &target)?;
    assert_eq!(fs::read(&target)?, b"fixture contents");
    assert!(extract(&archive, &target).is_err());
    fs::remove_file(target)?;
    fs::remove_file(archive)?;
    fs::remove_dir(root)?;
    Ok(())
}
#[test]
#[ignore = "Read-only network verification against the public GitHub release"]
fn github_check_and_published_archive_verify_without_installing() -> Result<()> {
    let check = check()?;
    println!(
        "latest={}, available={}, portable={}",
        check.latest,
        check.update.is_some(),
        check.portable
    );
    // Treat the latest real release as newer only to exercise asset parsing and
    // download verification. Do not call prepare/install or execute its binary.
    let bytes = bounded_bytes(
        agent()
            .get(API)
            .set("Accept", "application/vnd.github+json")
            .set("X-GitHub-Api-Version", "2022-11-28")
            .timeout(Duration::from_secs(20))
            .call()?
            .into_reader(),
        1024 * 1024,
    )?;
    let release = parse_release(&bytes, "0.0.0")?
        .update
        .context("No published stable asset")?;
    let root = std::env::temp_dir().join(format!(
        "shitu-network-update-{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    ));
    fs::create_dir(&root)?;
    let prepared = Prepared {
        directory: root,
        keep: false,
    };
    let hash = bounded_bytes(
        agent()
            .get(&release.checksum.browser_download_url)
            .timeout(Duration::from_secs(30))
            .call()?
            .into_reader(),
        4096,
    )?;
    let bytes = bounded_bytes(
        agent()
            .get(&release.archive.browser_download_url)
            .timeout(Duration::from_secs(90))
            .call()?
            .into_reader(),
        MAX_ARCHIVE,
    )?;
    assert_eq!(bytes.len() as u64, release.archive.size);
    let archive = prepared.directory.join("archive.zip");
    fs::write(&archive, bytes)?;
    ensure!(
        file_hash(&archive)? == checksum(&hash, &release.archive.name)?,
        "Published checksum mismatch"
    );
    let binary = prepared.directory.join("new.exe");
    extract(&archive, &binary)?;
    install::validate_binary(&binary)?;
    println!(
        "verified archive={}, bytes={}",
        release.archive.name, release.archive.size
    );
    Ok(())
}
