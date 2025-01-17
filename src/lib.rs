#![doc = include_str!("../README.md")]

use bevy::{
    asset::io::{AssetReader, AssetReaderError, AssetSource, AssetSourceId, PathStream, Reader},
    prelude::*,
    utils::BoxedFuture,
};
use std::path::{Path, PathBuf};

use std::pin::Pin;
use std::task::Poll;

/// A custom asset reader implementation that wraps a given asset reader implementation
pub struct HttpAssetReader {
    client: surf::Client,
    /// A random sequence that is interpreted as a slash. Used to work around
    /// the fact that bevy treats slashes as directories and will subsequently
    /// try to load sub-entities from the directory.
    fake_slash: String,
}

impl HttpAssetReader {
    /// Creates a new `HttpAssetReader`. The path provided will be used to build URLs to query for assets.
    pub fn new(base_url: &str, fake_slash: String) -> Self {
        let base_url = surf::Url::parse(base_url).expect("invalid base url");

        let client = surf::Config::new().set_timeout(Some(std::time::Duration::from_secs(5)));
        let client = client.set_base_url(base_url);

        let client = client.try_into().expect("could not create http client");

        Self { client, fake_slash }
    }

    async fn fetch_bytes<'a>(&self, path: &str) -> Result<Box<Reader<'a>>, AssetReaderError> {
        let resp = self.client.get(path).await;

        trace!("fetched {resp:?} ... ");
        let mut resp = resp.map_err(|e| {
            AssetReaderError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("error fetching {path}: {e}"),
            ))
        })?;

        let status = resp.status();

        if !status.is_success() {
            let err = match status {
                surf::StatusCode::NotFound => AssetReaderError::NotFound(path.into()),
                _ => AssetReaderError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("bad status code: {status}"),
                )),
            };
            return Err(err);
        };

        let bytes = resp.body_bytes().await.map_err(|e| {
            AssetReaderError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("error getting bytes for {path}: {e}"),
            ))
        })?;
        let reader = bevy::asset::io::VecReader::new(bytes);
        Ok(Box::new(reader))
    }
}

struct EmptyPathStream;

impl futures_core::Stream for EmptyPathStream {
    type Item = PathBuf;

    fn poll_next(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        Poll::Ready(None)
    }
}

impl AssetReader for HttpAssetReader {
    fn read<'a>(
        &'a self,
        path: &'a Path,
    ) -> BoxedFuture<'a, Result<Box<Reader<'a>>, AssetReaderError>> {
        let path = path.display().to_string().replace(&self.fake_slash, "/");
        Box::pin(async move { self.fetch_bytes(&path).await })
    }

    fn read_meta<'a>(
        &'a self,
        path: &'a Path,
    ) -> BoxedFuture<'a, Result<Box<Reader<'a>>, AssetReaderError>> {
        Box::pin(async move {
            let path = path.display().to_string().replace(&self.fake_slash, "/");
            let meta_path = path + ".meta";
            Ok(self.fetch_bytes(&meta_path).await?)
        })
    }

    fn read_directory<'a>(
        &'a self,
        _path: &'a Path,
    ) -> BoxedFuture<'a, Result<Box<PathStream>, AssetReaderError>> {
        let stream: Box<PathStream> = Box::new(EmptyPathStream);
        error!("Reading directories is not supported with the HttpAssetReader");
        Box::pin(async move { Ok(stream) })
    }

    fn is_directory<'a>(
        &'a self,
        _path: &'a Path,
    ) -> BoxedFuture<'a, std::result::Result<bool, AssetReaderError>> {
        error!("Reading directories is not supported with the HttpAssetReader");
        Box::pin(async move { Ok(false) })
    }
}

/// A plugins that registers the `HttpAssetReader` as an asset source.
pub struct HttpAssetReaderPlugin {
    pub id: String,
    pub base_url: String,
    /// A random sequence that is interpreted as a slash. Used to work around
    /// the fact that bevy treats slashes as directories and will subsequently
    /// try to load sub-entities from the directory.
    pub fake_slash: String,
}

impl Plugin for HttpAssetReaderPlugin {
    fn build(&self, app: &mut App) {
        let id = self.id.clone();
        let base_url = self.base_url.clone();
        let fake_slash = self.fake_slash.clone();
        app.register_asset_source(
            AssetSourceId::Name(id.into()),
            AssetSource::build()
                .with_reader(move || Box::new(HttpAssetReader::new(&base_url, fake_slash.clone()))),
        );
    }
}
