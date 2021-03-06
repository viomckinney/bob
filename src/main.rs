// bob - Docker image build agent
// Copyright (C) 2022 Violet McKinney <opensource@viomck.com>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

mod discord_log;
mod docker;
mod github;
mod store;
mod webhook;

#[macro_use]
extern crate lazy_static;

use crate::github::{GitHub, GitHubInfo};
use crate::webhook::WebHook;
use std::fs::File;
use std::path::Path;
use std::time::Duration;
use std::{env, fs, thread};

#[tokio::main]
async fn main() {
    if let Err(err) = dotenv::dotenv() {
        eprintln!("WARN Error initializing .env: {} (if you don't have a .env file, this does not matter)", err);
    }

    env::set_current_dir("./workspace").unwrap();

    println!("INFO Checking Docker config");
    docker::login();

    println!("INFO Checking GitHub config");
    let gh = GitHub::init();
    gh.ensure_config().await;

    println!("INFO Initializing store");
    store::ensure_store_exists();

    println!("INFO Initializing webhooks");
    let wh = webhook::WebHook::new();

    println!("INFO Checking Discord Log config");
    discord_log::hello_world().await;

    loop {
        println!("INFO Checking users...");

        for ghi in gh
            .get_watched_repo_info()
            .await
            .into_iter()
            .filter(store::is_newer_than_stored_sha)
        {
            process(ghi, &wh).await;
        }

        // 1 req per min is max github unauthenticated amount.  give some buffer
        thread::sleep(Duration::from_secs(60));
    }
}

async fn process(ghi: GitHubInfo, wh: &WebHook) {
    discord_log::log(&format!(
        "INFO Going on {}/{}!",
        ghi.repo_owner, ghi.repo_name
    ))
    .await;

    let repo_path = format!("{} {}", ghi.repo_owner, ghi.repo_name);

    fs::create_dir(&repo_path).unwrap();

    if let Err(err) = git2::Repository::clone(
        &format!("https://github.com/{}/{}", ghi.repo_owner, ghi.repo_name),
        &repo_path,
    ) {
        discord_log::log(&format!("ERROR Could not clone {}: {}", &repo_path, err)).await;
        return;
    }

    if let Err(err) = docker::build_and_push(&repo_path, &ghi.bob_tag) {
        discord_log::log(&format!(
            "ERROR Could not build image {}: {}",
            &ghi.bob_tag, err
        ))
        .await;
        return;
    }

    discord_log::log(&format!(
        "INFO **[SUCCESS]** Built image {} successfully",
        &ghi.bob_tag
    ))
    .await;

    wh.success(&ghi.repo_name, &ghi.repo_owner).await;

    fs::remove_dir_all(repo_path).unwrap();

    store::store_new_sha(&ghi);
}
