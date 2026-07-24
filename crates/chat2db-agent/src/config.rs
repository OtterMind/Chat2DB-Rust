use std::{fmt, str::FromStr, time::Duration};

use reqwest::{
    Client,
    header::{HeaderMap, HeaderName, HeaderValue},
    redirect::Policy,
};
use url::Url;
use zeroize::Zeroize;

use crate::ConfigError;

const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// An API credential that is never serialized or printed by `Debug`.
pub struct ApiKey(String);

impl ApiKey {
    /// Creates a non-empty credential.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::EmptyApiKey`] for an empty or whitespace-only value.
    pub fn new(value: impl Into<String>) -> Result<Self, ConfigError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ConfigError::EmptyApiKey);
        }
        Ok(Self(value))
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl Clone for ApiKey {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ApiKey([REDACTED])")
    }
}

impl Drop for ApiKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// Shared HTTP configuration for direct provider adapters.
#[derive(Clone)]
pub struct HttpProviderConfig {
    pub(crate) base_url: Url,
    pub(crate) model: String,
    pub(crate) api_key: ApiKey,
    pub(crate) headers: HeaderMap,
    pub(crate) connect_timeout: Duration,
    pub(crate) request_timeout: Duration,
}

impl HttpProviderConfig {
    /// Creates configuration with conservative connection and request timeouts.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid or unsafe base URL or an empty model.
    pub fn new(
        base_url: impl AsRef<str>,
        model: impl Into<String>,
        api_key: ApiKey,
    ) -> Result<Self, ConfigError> {
        let model = model.into().trim().to_owned();
        if model.trim().is_empty() {
            return Err(ConfigError::EmptyModel);
        }

        let mut base_url = Url::parse(base_url.as_ref())?;
        if !matches!(base_url.scheme(), "http" | "https") {
            return Err(ConfigError::UnsupportedBaseUrlScheme);
        }
        if base_url.host_str().is_none() {
            return Err(ConfigError::BaseUrlHost);
        }
        if !base_url.username().is_empty() || base_url.password().is_some() {
            return Err(ConfigError::BaseUrlCredentials);
        }
        if base_url.query().is_some() {
            return Err(ConfigError::BaseUrlQuery);
        }
        if base_url.fragment().is_some() {
            return Err(ConfigError::BaseUrlFragment);
        }
        if !base_url.path().ends_with('/') {
            let mut path = base_url.path().to_owned();
            path.push('/');
            base_url.set_path(&path);
        }

        Ok(Self {
            base_url,
            model,
            api_key,
            headers: HeaderMap::new(),
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            request_timeout: DEFAULT_REQUEST_TIMEOUT,
        })
    }

    /// Adds a custom header. Values are marked sensitive before insertion.
    ///
    /// # Errors
    ///
    /// Returns an error when the header name or value is invalid.
    pub fn with_header(mut self, name: &str, value: &str) -> Result<Self, ConfigError> {
        let name = HeaderName::from_str(name)
            .map_err(|_| ConfigError::InvalidHeaderName(name.to_owned()))?;
        let mut value = HeaderValue::from_str(value)
            .map_err(|_| ConfigError::InvalidHeaderValue(name.to_string()))?;
        value.set_sensitive(true);
        self.headers.insert(name, value);
        Ok(self)
    }

    /// Sets the TCP connection timeout.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::ZeroTimeout`] when `timeout` is zero.
    pub fn with_connect_timeout(mut self, timeout: Duration) -> Result<Self, ConfigError> {
        if timeout.is_zero() {
            return Err(ConfigError::ZeroTimeout);
        }
        self.connect_timeout = timeout;
        Ok(self)
    }

    /// Sets the end-to-end request timeout, including response streaming.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::ZeroTimeout`] when `timeout` is zero.
    pub fn with_request_timeout(mut self, timeout: Duration) -> Result<Self, ConfigError> {
        if timeout.is_zero() {
            return Err(ConfigError::ZeroTimeout);
        }
        self.request_timeout = timeout;
        Ok(self)
    }

    #[must_use]
    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    pub(crate) fn client(&self) -> Result<Client, ConfigError> {
        Client::builder()
            .redirect(Policy::none())
            .connect_timeout(self.connect_timeout)
            .timeout(self.request_timeout)
            .default_headers(self.headers.clone())
            .build()
            .map_err(|error| ConfigError::HttpClient(error.to_string()))
    }

    pub(crate) fn endpoint(&self, relative: &str) -> Result<Url, ConfigError> {
        self.base_url
            .join(relative)
            .map_err(ConfigError::InvalidBaseUrl)
    }
}

impl fmt::Debug for HttpProviderConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpProviderConfig")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("api_key", &self.api_key)
            .field("custom_header_count", &self.headers.len())
            .field("connect_timeout", &self.connect_timeout)
            .field("request_timeout", &self.request_timeout)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{ApiKey, HttpProviderConfig};

    #[test]
    fn debug_output_never_contains_api_key_or_header_values() {
        let config = HttpProviderConfig::new(
            "https://example.test/v1",
            "model",
            ApiKey::new("top-secret").expect("valid key"),
        )
        .expect("valid config")
        .with_header("x-tenant-secret", "also-secret")
        .expect("valid header");

        let debug = format!("{config:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("top-secret"));
        assert!(!debug.contains("also-secret"));
        assert_eq!(config.base_url().as_str(), "https://example.test/v1/");
    }

    #[test]
    fn base_url_rejects_credentials_query_fragment_and_non_http_schemes() {
        for url in [
            "https://user:password@example.test/v1",
            "https://example.test/v1?tenant=secret",
            "https://example.test/v1#secret",
            "file:///tmp/provider",
        ] {
            let error =
                HttpProviderConfig::new(url, "model", ApiKey::new("key").expect("valid key"))
                    .expect_err("unsafe base URL must fail");
            let debug = format!("{error:?}");
            assert!(!debug.contains("password"));
            assert!(!debug.contains("tenant=secret"));
            assert!(!debug.contains("#secret"));
        }
    }

    #[test]
    fn whitespace_only_secrets_and_models_are_rejected() {
        assert!(ApiKey::new(" \n\t ").is_err());
        assert!(
            HttpProviderConfig::new(
                "https://example.test/v1",
                "  ",
                ApiKey::new("key").expect("valid key"),
            )
            .is_err()
        );
    }
}
