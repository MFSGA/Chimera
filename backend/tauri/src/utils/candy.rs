use super::config::NyanpasuReqwestProxyExt;
use anyhow::Result;
use futures_util::future;
use std::time::Duration;
use url::Url;

pub fn get_reqwest_client() -> Result<reqwest::Client> {
    let client = reqwest::ClientBuilder::new()
        .swift_set_nyanpasu_proxy()
        .user_agent(format!(
            "clash-chimera/{}",
            crate::consts::BUILD_INFO.pkg_version
        ))
        .build()?;
    Ok(client)
}

pub const INTERNAL_MIRRORS: &[&str] = &["https://github.com/", "https://gh-proxy.com/"];

pub fn parse_gh_url(mirror: &str, path: &str) -> Result<Url, url::ParseError> {
    if mirror.contains("github.com") && !path.starts_with('/') {
        Url::parse(path)
    } else {
        let mut url = Url::parse(mirror)?;
        url.set_path(path);
        Ok(url)
    }
}

#[async_trait::async_trait]
pub trait ReqwestSpeedTestExt {
    async fn mirror_speed_test<'a>(
        &self,
        mirrors: &'a [&'a str],
        path: &'a str,
    ) -> Result<Vec<(&'a str, f64)>>;
}

#[async_trait::async_trait]
impl ReqwestSpeedTestExt for reqwest::Client {
    async fn mirror_speed_test<'a>(
        &self,
        mirrors: &'a [&'a str],
        path: &'a str,
    ) -> Result<Vec<(&'a str, f64)>> {
        let results = future::join_all(mirrors.iter().map(|&mirror| {
            let client = self;
            async move {
                let start = tokio::time::Instant::now();
                let url = parse_gh_url(mirror, path)?;
                let _ =
                    tokio::time::timeout(Duration::from_secs(3), client.get(url.as_str()).send())
                        .await;
                let result: Result<reqwest::Response, anyhow::Error> =
                    tokio::time::timeout(Duration::from_secs(3), client.get(url).send())
                        .await
                        .map_err(anyhow::Error::msg)
                        .and_then(|v| v.map_err(anyhow::Error::msg))
                        .and_then(|v| v.error_for_status().map_err(anyhow::Error::msg));
                match result {
                    Ok(response) => {
                        let content_length = response.content_length().unwrap_or(0) as f64;
                        match response.bytes().await {
                            Ok(_) => Ok((mirror, content_length / start.elapsed().as_secs_f64())),
                            Err(err) => {
                                tracing::warn!("test mirror {} failed: {}", mirror, err);
                                Ok((mirror, 0.0))
                            }
                        }
                    }
                    Err(err) => {
                        tracing::warn!("test mirror {} failed: {}", mirror, err);
                        Ok((mirror, 0.0))
                    }
                }
            }
        }))
        .await;

        let collected: Result<Vec<_>, anyhow::Error> = results.into_iter().collect();
        let mut results = collected?;
        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        Ok(results)
    }
}
