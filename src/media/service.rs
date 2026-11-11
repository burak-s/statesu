//! Media domain rules: connection creation and rotation with verification,
//! per-blog storage preference, ownership across every mutation. Credential
//! secrets are sealed immediately; the factory determines verification and
//! runtime object access, so the service stays provider-agnostic.
//!
//! Durability contract: every cleanup job commits (with all object keys and
//! the sealed connection material) *before* any remote side effect, so
//! crashed requests, restarts and provider outages can always make it back
//! to a clean state through background maintenance.

use std::sync::Arc;

use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::blog::repo::BlogRepository;
use crate::clock::Clock;
use crate::crypto::secret::SecretCipher;
use crate::error::{Error, InternalError};
use crate::ids::parse_uuid_v7;
use crate::media::media_repo::{
    Finalize, MediaFingerprint, MediaInsert, MediaJobInsert, MediaJobRow, MediaRepository,
    MediaRow, MediaVariantInsert, MediaVariantRow,
};
use crate::media::process::{self, Processed};
use crate::media::repo::{StorageConnectionInsert, StorageConnectionRepository};
use crate::media::{
    BlogStorageResponse, CreateConnectionRequest, MAX_CONNECTIONS_PER_BLOG, MAX_CREDENTIAL_BYTES,
    MAX_NAME_BYTES, ReplaceStorageRequest, StorageConnectionResponse, StorageConnectionRow,
    UpdateConnectionRequest, connection_material, normalize_prefix, validate_account_id,
    validate_bucket_name, validate_public_base_url,
};
use crate::pagination::{parse_page_number, parse_page_size};
use crate::post::service::ListResponse;
use crate::storage::{BucketConnection, StorageFactory};

/// Durability budget for cleanup retries: capped backoff keeps failures
/// visible without unbounded background cost.
const MAX_JOB_ATTEMPTS: i64 = 8;
const JOB_BACKOFF_BASE_SECS: i64 = 30;
const JOB_BACKOFF_CAP_SECS: i64 = 3600;
/// An upload stuck in `uploading` for this long died mid-request; its
/// pre-registered job fires at this horizon. This sits far outside the
/// HTTP upload timeout, so a healthy request always removes its own job
/// before it ever comes due.
const STALE_UPLOAD_AFTER_SECS: i64 = 300;

/// Codec work is CPU-heavy and synchronous; it runs on the blocking pool
/// under a small semaphore so a burst of uploads degrades throughput, not
/// the whole runtime.
const PIPELINE_PERMITS_MAX: usize = 4;

/// The server-owned bucket, mirrored from config when present.
#[derive(Debug, Clone)]
pub struct ServerStorage {
    pub connection: BucketConnection,
    pub public_base_url: String,
}

/// Media rules injected with repositories, the secret cipher, one storage
/// factory and the clock; no globals.
#[derive(Clone)]
pub struct MediaService {
    connections: StorageConnectionRepository,
    media: MediaRepository,
    blogs: BlogRepository,
    secrets: SecretCipher,
    factory: Arc<dyn StorageFactory>,
    clock: Arc<dyn Clock>,
    server_storage: Option<ServerStorage>,
    max_media_bytes: usize,
    max_total_media_bytes: usize,
    max_media_per_user: usize,
    pipeline: Arc<Semaphore>,
}

impl MediaService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        connections: StorageConnectionRepository,
        media: MediaRepository,
        blogs: BlogRepository,
        secrets: SecretCipher,
        factory: Arc<dyn StorageFactory>,
        clock: Arc<dyn Clock>,
        server_storage: Option<ServerStorage>,
        max_media_bytes: usize,
        max_total_media_bytes: usize,
        max_media_per_user: usize,
    ) -> MediaService {
        let permits = std::thread::available_parallelism()
            .map(|parallelism| parallelism.get())
            .unwrap_or(1)
            .clamp(1, PIPELINE_PERMITS_MAX);
        MediaService {
            connections,
            media,
            blogs,
            secrets,
            factory,
            clock,
            server_storage,
            max_media_bytes,
            max_total_media_bytes,
            max_media_per_user,
            pipeline: Arc::new(Semaphore::new(permits)),
        }
    }

    /// Where new media on this blog lands: the blog's custom connection or
    /// the server's own bucket. Storage absence is a configuration state,
    /// not a user fault.
    pub(crate) async fn storage_for_blog(
        &self,
        owner_id: &Uuid,
        blog_id: &Uuid,
    ) -> Result<ResolvedStorage, Error> {
        match self.connections.blog_connection(blog_id).await? {
            Some(row) => {
                if row.owner_id != *owner_id {
                    return Err(Error::BlogNotFound);
                }
                let connection = connection_material(&row, &self.secrets)?;
                Ok(ResolvedStorage {
                    connection,
                    public_base_url: row.public_base_url.clone(),
                    connection_id: Some(row.id),
                })
            }
            None => match &self.server_storage {
                Some(server) => Ok(ResolvedStorage {
                    connection: server.connection.clone(),
                    public_base_url: server.public_base_url.clone(),
                    connection_id: None,
                }),
                None => Err(Error::Forbidden),
            },
        }
    }

    /// Blog ownership is verified before any media mutation; a foreign or
    /// unknown blog is always `BlogNotFound`.
    async fn owned_blog(&self, owner_id: &Uuid, blog_id_param: &str) -> Result<Uuid, Error> {
        let blog_id = parse_uuid_v7(blog_id_param)?;
        self.blogs
            .find_for_owner(owner_id, &blog_id)
            .await?
            .ok_or(Error::BlogNotFound)?;
        Ok(blog_id)
    }

    pub async fn create_connection(
        &self,
        owner_id: &Uuid,
        blog_id_param: &str,
        request: &CreateConnectionRequest,
    ) -> Result<StorageConnectionResponse, Error> {
        let blog_id = self.owned_blog(owner_id, blog_id_param).await?;
        let count = self.connections.count_for_blog(&blog_id).await?;
        if count >= i64::try_from(MAX_CONNECTIONS_PER_BLOG).unwrap_or(i64::MAX) {
            return Err(Error::Forbidden);
        }
        let bucket_connection = self.validated_connection(
            &request.account_id,
            &request.bucket,
            request.prefix.as_deref(),
            &request.public_base_url,
            &request.access_key_id,
            &request.secret_key,
        )?;
        let public_base_url = validate_public_base_url(&request.public_base_url)?;
        self.factory
            .verify_connection(&bucket_connection, &public_base_url)
            .await?;
        let now = self.clock.now();
        let id = Uuid::now_v7();
        let sealed_secret = self.secrets.seal(request.secret_key.as_bytes(), &id)?;
        let row = StorageConnectionRow {
            id,
            blog_id,
            owner_id: *owner_id,
            name: valid_name(request.name.as_deref())?,
            account_id: bucket_connection.account_id.clone(),
            bucket: bucket_connection.bucket.clone(),
            prefix: if bucket_connection.prefix.is_empty() {
                None
            } else {
                Some(bucket_connection.prefix.clone())
            },
            public_base_url,
            access_key_id: bucket_connection.access_key_id.clone(),
            secret_key: sealed_secret,
            verified_at: now,
            created_at: now,
            updated_at: now,
        };
        self.connections.insert(&insert_of(&row)).await?;
        Ok(row.to_response())
    }

    pub async fn list_connections(
        &self,
        owner_id: &Uuid,
        blog_id_param: &str,
    ) -> Result<Vec<StorageConnectionResponse>, Error> {
        let blog_id = self.owned_blog(owner_id, blog_id_param).await?;
        let rows = self.connections.list_for_blog(owner_id, &blog_id).await?;
        Ok(rows.iter().map(StorageConnectionRow::to_response).collect())
    }

    /// Renames, rotates credentials, or both after a fresh verification of
    /// the *new* credentials: an already-revoked old key cannot strand the
    /// connection, and an unverified replacement cannot be saved over a
    /// working one.
    pub async fn update_connection(
        &self,
        owner_id: &Uuid,
        blog_id_param: &str,
        connection_id_param: &str,
        request: &UpdateConnectionRequest,
    ) -> Result<StorageConnectionResponse, Error> {
        let blog_id = self.owned_blog(owner_id, blog_id_param).await?;
        let connection_id = parse_uuid_v7(connection_id_param)?;
        let row = self
            .connections
            .find_for_owner(owner_id, &blog_id, &connection_id)
            .await?
            .ok_or(Error::BlogNotFound)?;
        let now = self.clock.now();
        match (&request.access_key_id, &request.secret_key) {
            (Some(access_key_id), Some(secret_key)) => {
                if access_key_id.is_empty()
                    || access_key_id.len() > MAX_CREDENTIAL_BYTES
                    || secret_key.is_empty()
                    || secret_key.len() > MAX_CREDENTIAL_BYTES
                {
                    return Err(Error::InvalidInput);
                }
                let mut candidate = connection_material(&row, &self.secrets)?;
                candidate.access_key_id = access_key_id.clone();
                candidate.secret_key = secret_key.clone();
                self.factory
                    .verify_connection(&candidate, &row.public_base_url)
                    .await?;
                let sealed = self.secrets.seal(secret_key.as_bytes(), &row.id)?;
                self.connections
                    .replace_credentials(&crate::media::repo::CredentialRotation {
                        id: &row.id,
                        blog_id: &blog_id,
                        owner_id,
                        access_key_id,
                        sealed_secret: &sealed,
                        verified_at: now,
                        updated_at: now,
                    })
                    .await?;
            }
            (None, None) => {}
            _ => return Err(Error::InvalidInput),
        }
        let name = match &request.name {
            Some(name) => {
                let name = valid_name(Some(name))?;
                self.connections
                    .rename(&row.id, &blog_id, owner_id, name.as_deref(), now)
                    .await?;
                name
            }
            None => row.name.clone(),
        };
        Ok(StorageConnectionResponse {
            name,
            verified_at: if request.access_key_id.is_some() {
                now
            } else {
                row.verified_at
            },
            updated_at: now,
            ..row.to_response()
        })
    }

    /// A connection must persist media references, so in-use connections
    /// refuse deletion and keep serving old media.
    pub async fn delete_connection(
        &self,
        owner_id: &Uuid,
        blog_id_param: &str,
        connection_id_param: &str,
    ) -> Result<(), Error> {
        let blog_id = self.owned_blog(owner_id, blog_id_param).await?;
        let connection_id = parse_uuid_v7(connection_id_param)?;
        self.connections
            .delete(&connection_id, &blog_id, owner_id)
            .await
    }

    pub async fn blog_storage(
        &self,
        owner_id: &Uuid,
        blog_id_param: &str,
    ) -> Result<BlogStorageResponse, Error> {
        let blog_id = self.owned_blog(owner_id, blog_id_param).await?;
        match self.connections.blog_connection(&blog_id).await? {
            Some(row) => Ok(BlogStorageResponse {
                storage: "custom".to_owned(),
                connection_id: Some(row.id),
            }),
            None => Ok(BlogStorageResponse {
                storage: "statesu".to_owned(),
                connection_id: None,
            }),
        }
    }

    /// New media on this blog will use the referenced connection or the
    /// server bucket. Existing media never re-parents.
    pub async fn set_blog_storage(
        &self,
        owner_id: &Uuid,
        blog_id_param: &str,
        request: &ReplaceStorageRequest,
    ) -> Result<BlogStorageResponse, Error> {
        let blog_id = self.owned_blog(owner_id, blog_id_param).await?;
        match request.connection_id {
            Some(connection_id) => {
                let row = self
                    .connections
                    .find_for_owner(owner_id, &blog_id, &connection_id)
                    .await?
                    .ok_or(Error::BlogNotFound)?;
                self.connections
                    .set_blog_storage(&blog_id, &row.id, self.clock.now())
                    .await?;
            }
            None => self.connections.clear_blog_storage(&blog_id).await?,
        }
        self.blog_storage(owner_id, blog_id_param).await
    }

    /// Storage for one media row, resolved from the row's own connection,
    /// never the current blog preference. `None` means the row's storage is
    /// no longer available (deleted connection or removed server bucket).
    async fn storage_for_row(
        &self,
        owner_id: &Uuid,
        blog_id: &Uuid,
        connection_id: Option<Uuid>,
    ) -> Option<ResolvedStorage> {
        match connection_id {
            Some(connection_id) => {
                let row = self
                    .connections
                    .find_for_owner(owner_id, blog_id, &connection_id)
                    .await
                    .ok()??;
                let connection = connection_material(&row, &self.secrets).ok()?;
                Some(ResolvedStorage {
                    connection,
                    public_base_url: row.public_base_url.clone(),
                    connection_id: Some(row.id),
                })
            }
            None => {
                let server = self.server_storage.as_ref()?;
                Some(ResolvedStorage {
                    connection: server.connection.clone(),
                    public_base_url: server.public_base_url.clone(),
                    connection_id: None,
                })
            }
        }
    }

    /// Uploads and processes one image; on success the record is `ready`
    /// and the response carries the public URL that Markdown will embed.
    ///
    /// Order matters: reserve the row, commit the durable cleanup job with
    /// every object path plus sealed connection material, then (and only
    /// then) write objects remotely. Finalization is one transaction for
    /// the row and all its variants, and the job is released last, so no
    /// intermediate request state is ever invisible to maintenance.
    pub async fn upload(
        &self,
        owner_id: &Uuid,
        blog_id_param: &str,
        body: &[u8],
    ) -> Result<MediaResponse, Error> {
        if body.is_empty() {
            return Err(Error::InvalidInput);
        }
        if body.len() > self.max_media_bytes {
            return Err(Error::PayloadTooLarge);
        }
        let blog_id = self.owned_blog(owner_id, blog_id_param).await?;
        let sniffed = process::sniff(body)?;
        let storage = self.storage_for_blog(owner_id, &blog_id).await?;

        let now = self.clock.now();
        let id = Uuid::now_v7();
        // Server-bucket rows pin their location identity (never secrets:
        // server R2 credentials stay in the environment, per plan §4) so
        // later config changes never reroute old objects. Custom rows
        // live in their (deletion-blocked) connection rows. The blob
        // travels into the ready transition and every later cleanup.
        let server_location: Option<Vec<u8>> = if storage.connection_id.is_none() {
            Some(
                serde_json::to_vec(&ServerLocation::of(&storage))
                    .map_err(|e| internal_io(&e.to_string()))?,
            )
        } else {
            None
        };
        // Four inputs is generous for one cleaned source plus three
        // downscales, every one smaller than the source; the cap keeps the
        // estimate alone from exceeding the user budget.
        let max_total = i64::try_from(self.max_total_media_bytes).unwrap_or(i64::MAX);
        let estimate = (i64::try_from(body.len()).unwrap_or(0).saturating_mul(4)).min(max_total);
        self.media
            .reserve(
                &MediaInsert {
                    id,
                    blog_id,
                    owner_id: *owner_id,
                    connection_id: storage.connection_id,
                    public_base_url: storage.public_base_url.clone(),
                    mime: sniffed.mime.to_owned(),
                    reserved_bytes: estimate,
                    now,
                },
                max_total,
                i64::try_from(self.max_media_per_user).unwrap_or(i64::MAX),
            )
            .await?;

        // Object keys are absolute: the connection prefix is joined here,
        // once, and stored or deleted verbatim from then on.
        let source_key = storage.connection.object_path(&format!(
            "blogs/{blog_id}/{id}/source.{}",
            sniffed.extension()
        ));
        let variant_keys: Vec<(String, u32)> = process::VARIANT_WIDTHS
            .iter()
            .map(|width| {
                (
                    storage.connection.object_path(&format!(
                        "blogs/{blog_id}/{id}/w{width}.{}",
                        sniffed.extension()
                    )),
                    *width,
                )
            })
            .collect();
        let cleanup_keys: Vec<String> = std::iter::once(source_key.clone())
            .chain(variant_keys.iter().map(|(key, _)| key.clone()))
            .collect();

        let job_id = self
            .schedule_cleanup(
                owner_id,
                CleanupMode::Upload,
                id,
                blog_id,
                &storage,
                cleanup_keys,
                now + STALE_UPLOAD_AFTER_SECS,
            )
            .await?;

        // Heavy codecs run off the request task, throttled by permits so
        // concurrent uploads cannot starve the rest of the runtime.
        let processed = {
            let pipeline = self.pipeline.clone();
            let permit = pipeline.acquire_owned().await.map_err(|e| {
                Error::Internal(InternalError::new(std::io::Error::other(e.to_string())))
            })?;
            let bytes_owned = body.to_vec();
            match tokio::task::spawn_blocking(move || {
                let _permit = permit;
                process::process(sniffed, &bytes_owned)
            })
            .await
            {
                Ok(Ok(processed)) => processed,
                // Decode/encode rejection: a bad user file, not a fault.
                Ok(Err(decode_error)) => {
                    self.abandon_upload(job_id, id, now, "upload processing failed")
                        .await;
                    return Err(decode_error);
                }
                Err(join_error) => {
                    self.abandon_upload(job_id, id, now, "upload processing task died")
                        .await;
                    return Err(internal_io(&join_error.to_string()));
                }
            }
        };

        let store = self.factory.store_for(&storage.connection)?;
        let mut put_error: Option<String> = None;
        if let Err(err) = store.put(&source_key, &processed.source.bytes).await {
            put_error.get_or_insert(err.to_string());
        }
        for variant in &processed.variants {
            if put_error.is_some() {
                break;
            }
            let Some((key, _)) = variant_keys
                .iter()
                .find(|(_, width)| *width == variant.width)
            else {
                continue;
            };
            if let Err(err) = store.put(key, &variant.encoded.bytes).await {
                put_error.get_or_insert(err.to_string());
            }
        }
        if let Some(put_error) = put_error {
            self.abandon_upload(job_id, id, now, &put_error).await;
            return Err(internal_io(&format!("object put failed: {put_error}")));
        }

        // Objects are in place: the row, its variants and its public base
        // commit together, and only then does the durable job go.
        let variant_inserts = variant_inserts_for(id, &processed, &source_key, &variant_keys);
        let finalize = Finalize {
            id: &id,
            owner_id,
            mime: processed.source.mime,
            width: processed.width,
            height: processed.height,
            stored_bytes: processed.total_bytes(),
            max_total_bytes: max_total,
            exempt_from_quota: storage.connection_id.is_some(),
            storage_location: server_location.as_deref(),
            now: self.clock.now(),
        };
        if let Err(fail) = self
            .media
            .finalize_ready_with_variants(&finalize, &variant_inserts)
            .await
        {
            let detail = if matches!(fail, Error::Forbidden) {
                "processed output exceeds the user media budget"
            } else {
                "media finalize failed"
            };
            self.abandon_upload(job_id, id, now, detail).await;
            return Err(if matches!(fail, Error::Forbidden) {
                Error::Forbidden
            } else {
                internal_io("media finalize")
            });
        }
        self.media.delete_job(&job_id).await?;

        let public_url = crate::storage::format_public_url(&storage.public_base_url, &source_key);
        let url_for = |key: &str| crate::storage::format_public_url(&storage.public_base_url, key);
        Ok(MediaResponse {
            id,
            blog_id,
            status: "ready".to_owned(),
            mime: processed.source.mime.to_owned(),
            width: Some(processed.width),
            height: Some(processed.height),
            stored_bytes: processed.total_bytes(),
            created_at: now,
            public_url,
            variants: variant_inserts
                .iter()
                .filter(|insert| insert.label != "source")
                .map(|insert| MediaVariantResponse {
                    label: insert.label.clone(),
                    width: insert.width,
                    height: insert.height,
                    bytes: insert.bytes,
                    public_url: url_for(&insert.object_key),
                })
                .collect(),
        })
    }

    /// Releases a failed upload: quota reservation drops, the durable job
    /// is pulled forward so its inline delete pass — and retries — handle
    /// whatever objects already exist (paths that were never written
    /// delete as no-ops).
    async fn abandon_upload(&self, job_id: Uuid, media_id: Uuid, now: i64, reason: &str) {
        let _ = self.media.release(&media_id, now).await;
        self.media
            .schedule_retry(&job_id, 0, now + 1, &truncate_detail(reason))
            .await
            .ok();
    }

    /// One page of the blog's media, newest first. Publishing bases live
    /// on the rows as committed, so pages keep resolving after preference
    /// changes or a connection disappearing.
    pub async fn page_for_blog(
        &self,
        owner_id: &Uuid,
        blog_id_param: &str,
        page_param: Option<&str>,
        size_param: Option<&str>,
    ) -> Result<ListResponse<MediaResponse>, Error> {
        let blog_id = self.owned_blog(owner_id, blog_id_param).await?;
        let page = parse_page_number(page_param)?;
        let size = parse_page_size(size_param)?;
        let offset = u64::from(page.saturating_sub(1)) * u64::from(size);
        let rows = self
            .media
            .page_for_blog(
                &blog_id,
                i64::from(size),
                i64::try_from(offset).map_err(|_| Error::InvalidInput)?,
            )
            .await?;
        let total = self.media.count_for_blog(&blog_id).await?;
        let mut items = Vec::with_capacity(rows.len());
        for row in &rows {
            let variants = self.media.variants_for(&row.id).await?;
            items.push(media_response(row, &variants))
        }
        Ok(ListResponse {
            items,
            page,
            size,
            total,
        })
    }

    /// Deletes one media record and its objects. First the durable job
    /// takes over, the row flips to `deleting` (public reads drop it from
    /// that exact moment), then a best-effort inline pass runs; whatever
    /// the inline pass misses repeats through maintenance until gone.
    pub async fn delete_media(
        &self,
        owner_id: &Uuid,
        blog_id_param: &str,
        media_id_param: &str,
    ) -> Result<(), Error> {
        let blog_id = self.owned_blog(owner_id, blog_id_param).await?;
        let media_id = parse_uuid_v7(media_id_param)?;
        let row = self
            .media
            .find_for_owner(owner_id, &blog_id, &media_id)
            .await?
            .ok_or(Error::MediaNotFound)?;
        let variants = self.media.variants_for(&row.id).await?;
        let keys: Vec<String> = variants
            .iter()
            .map(|variant| variant.object_key.clone())
            .collect();
        let now = self.clock.now();
        // The row's own commit-time snapshot decides where the objects
        // live; the connection row (current preference) is only a fallback
        // for rows that predate a broken snapshot.
        let resolved = self.resolve_row_storage(&row).await;
        if resolved.is_none() {
            // Unresolvable storage is never a quiet success: the row
            // leaves the public surface at once and the object paths
            // stay tracked in a durable job that waits for the storage
            // identity to return, then finishes the deletion.
            let location = row
                .storage_location
                .as_deref()
                .and_then(|blob| serde_json::from_slice::<ServerLocation>(blob).ok());
            let job = self.unresolved_cleanup_job(
                owner_id,
                blog_id,
                CleanupMode::Delete,
                Some(row.id),
                row.connection_id,
                location,
                keys,
                now + 1,
            );
            self.media.insert_job(&job.to_insert()).await?;
            self.media
                .mark_for_deletion(&row.id, &blog_id, owner_id)
                .await?;
            return Ok(());
        }
        let storage = resolved.expect("checked");
        let job_id = self
            .schedule_cleanup(
                owner_id,
                CleanupMode::Delete,
                row.id,
                blog_id,
                &storage,
                keys.clone(),
                now + 1,
            )
            .await?;
        self.media
            .mark_for_deletion(&row.id, &blog_id, owner_id)
            .await?;
        {
            let store = self.factory.store_for(&storage.connection)?;
            let mut all_deleted = true;
            for key in &keys {
                if store.delete(key).await.is_err() {
                    all_deleted = false;
                }
            }
            if all_deleted {
                self.media.complete_deletion(&row.id, &job_id).await?;
            }
        }
        Ok(())
    }

    /// Public delivery metadata: for every referenced `ready` media record
    /// in the contents, the canonical source URL plus responsive variants.
    /// Markdown is never rewritten; the caller just attaches the result.
    /// Unresolvable bases drop out instead of failing public reads.
    pub async fn public_metadata(
        &self,
        blog_id: &Uuid,
        contents: &[&str],
    ) -> Result<Vec<MediaPublicMeta>, Error> {
        let ids = crate::media::referenced_media_ids(contents);
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let sources = self.media.public_ready_with_base(blog_id, &ids).await?;
        let variants = self.media.variants_for_many(&ids).await?;
        let mut variants_by_media: std::collections::HashMap<Uuid, Vec<&MediaVariantRow>> =
            std::collections::HashMap::new();
        for variant in &variants {
            variants_by_media
                .entry(variant.media_id)
                .or_default()
                .push(variant);
        }
        let mut result = Vec::new();
        for source in sources {
            let Some(variants) = variants_by_media.remove(&source.id) else {
                continue;
            };
            let Some(source_variant) = variants.iter().find(|variant| variant.label == "source")
            else {
                continue;
            };
            let Some(source_url) = crate::storage::format_public_url(
                &source.public_base_url,
                &source_variant.object_key,
            ) else {
                continue;
            };
            // Responsive variants order from smallest to largest width; the
            // largest doubles as the inspection surface.
            let mut responsive: Vec<(u32, &MediaVariantRow)> = variants
                .iter()
                .filter(|variant| variant.label != "source")
                .filter_map(|variant| {
                    variant
                        .label
                        .strip_prefix('w')
                        .and_then(|width| width.parse::<u32>().ok())
                        .map(|width| (width, *variant))
                })
                .collect();
            responsive.sort_by_key(|(width, _)| *width);
            let variants_of_url = |variant_key: &str| {
                crate::storage::format_public_url(&source.public_base_url, variant_key)
            };
            let zoom_url = match responsive.last() {
                Some((_, largest)) => variants_of_url(&largest.object_key),
                None => Some(source_url.clone()),
            };
            let Some(zoom_url) = zoom_url else {
                continue;
            };
            let mut shape = MediaPublicMeta {
                id: source.id,
                source_url,
                source_width: source_variant.width,
                source_height: source_variant.height,
                mime: source.mime,
                zoom_url,
                variants: Vec::with_capacity(responsive.len()),
            };
            for (_, variant) in responsive {
                let Some(url) = variants_of_url(&variant.object_key) else {
                    continue;
                };
                shape.variants.push(MediaPublicVariantMeta {
                    label: variant.label.clone(),
                    url,
                    width: variant.width,
                    height: variant.height,
                });
            }
            result.push(shape);
        }
        Ok(result)
    }

    /// Blog deletion can only legally run after this snapshot: the caller
    /// groups media by their committed storage references, then registers
    /// one cleanup job per distinct target — all resolved *before* the
    /// cascade erases the rows. Custom connections seal their row
    /// material (the connection cannot be deleted while referenced);
    /// server-bucket groups travel by location identity only, so their
    /// credentials re-resolve from current config at run time.
    pub async fn collect_blog_cleanup(
        &self,
        owner_id: Uuid,
        blog_id: Uuid,
    ) -> Result<(Vec<MediaFingerprint>, Vec<PreparedCleanupJob>), Error> {
        let now = self.clock.now();
        let rows = self.media.blog_media_cleanup(&owner_id, &blog_id).await?;
        let mut fingerprints: Vec<MediaFingerprint> = Vec::with_capacity(rows.len());
        let mut groups: std::collections::HashMap<
            String,
            (Option<BucketConnection>, ServerLocation, Vec<String>),
        > = std::collections::HashMap::new();
        let mut jobs: Vec<PreparedCleanupJob> = Vec::with_capacity(rows.len());
        for row in &rows {
            let variants = self.media.variants_for(&row.id).await?;
            fingerprints.push(MediaFingerprint {
                media_id: row.id,
                status: row.status.clone(),
                object_keys: variants
                    .iter()
                    .map(|variant| variant.object_key.clone())
                    .collect(),
            });
            let storage = self.resolve_row_storage(row).await;
            let Some(storage) = storage else {
                // Unresolvable storage: the cascade must still not lose
                // the object paths. An empty-credential job keeps them
                // durable — the run side treats it as a configuration
                // wait and reports it, and a returning config/row
                // (connection table reloads, server bucket back) drains
                // the job instead of leaving untracked objects.
                let location = row
                    .storage_location
                    .as_deref()
                    .and_then(|blob| serde_json::from_slice::<ServerLocation>(blob).ok());
                jobs.push(
                    self.unresolved_cleanup_job(
                        &owner_id,
                        blog_id,
                        CleanupMode::Blog,
                        None,
                        row.connection_id,
                        location,
                        variants
                            .iter()
                            .map(|variant| variant.object_key.clone())
                            .collect(),
                        now + 1,
                    ),
                );
                continue;
            };
            let identity = format!(
                "{}/{}/{}",
                storage.connection.account_id, storage.connection.bucket, storage.public_base_url
            );
            let custom = storage.connection_id.is_some();
            let connection = if custom {
                Some(storage.connection.clone())
            } else {
                None
            };
            let location = ServerLocation::of(&storage);
            let (_, _location_of_group, paths) =
                groups
                    .entry(identity)
                    .or_insert((connection.clone(), location, Vec::new()));
            paths.extend(variants.iter().map(|variant| variant.object_key.clone()));
        }
        for (connection, location, paths) in groups.into_values() {
            let job_id = Uuid::now_v7();
            let material = match &connection {
                Some(connection) => Some(JobStorageMaterial {
                    account_id: connection.account_id.clone(),
                    bucket: connection.bucket.clone(),
                    prefix: if connection.prefix.is_empty() {
                        None
                    } else {
                        Some(connection.prefix.clone())
                    },
                    public_base_url: location.public_base_url.clone(),
                    access_key_id: connection.access_key_id.clone(),
                    sealed_secret: self
                        .secrets
                        .seal(connection.secret_key.as_bytes(), &job_id)?,
                }),
                None => None,
            };
            // Only server-bucket groups carry the location identity;
            // custom groups carry their sealed material exclusively, so
            // the run side can never route them at the server bucket.
            let server_location = if connection.is_none() {
                Some(location)
            } else {
                None
            };
            jobs.push(PreparedCleanupJob {
                job_id,
                owner_id,
                blog_id,
                payload: serde_json::json!({
                    "mode": CleanupMode::Blog,
                    "paths": paths,
                    "blog_id": blog_id,
                    "media_id": serde_json::Value::Null,
                    "connection_id": serde_json::Value::Null,
                    "storage": material,
                    "server_location": server_location,
                })
                .to_string(),
                due_at: now + 1,
            });
        }
        Ok((fingerprints, jobs))
    }

    /// Blog deletion composite: ownership check, then snapshot, then one
    /// transaction that removes the blog and commits the cleanup jobs
    /// together — the bucket can never lose track of its bytes. A lost
    /// race (media mutated between snapshot and transaction) aborts with
    /// a retryable error instead of leaving untracked objects.
    pub async fn delete_blog_with_cleanup(
        &self,
        owner_id: Uuid,
        blog_id: Uuid,
    ) -> Result<(), Error> {
        self.blogs
            .find_for_owner(&owner_id, &blog_id)
            .await?
            .ok_or(Error::BlogNotFound)?;
        let (fingerprints, jobs) = self.collect_blog_cleanup(owner_id, blog_id).await?;
        let inserts: Vec<MediaJobInsert> =
            jobs.iter().map(|prepared| prepared.to_insert()).collect();
        self.blogs
            .delete_with_media_cleanup(&blog_id, &owner_id, &fingerprints, &inserts)
            .await
    }

    /// Background sweep: release dead uploads and drain due cleanup jobs.
    /// Idempotent per turn; failures stay visible through `last_error`.
    pub async fn run_maintenance(&self, now: i64) -> Result<(), Error> {
        // 1) Stale uploads: every upload carries a pre-registered cleanup
        // job, so these only need their quota reservation let go.
        for row in self
            .media
            .stale_uploading(now - STALE_UPLOAD_AFTER_SECS, 100)
            .await?
        {
            self.media.release(&row.id, now).await?;
        }

        // 2) Due cleanup jobs: one snapshot payload per job means storage
        // resolution never depends on rows, connections or config state.
        let jobs = self.media.due_jobs(now, MAX_JOB_ATTEMPTS).await?;
        for job in jobs {
            self.run_cleanup_job(&job, now).await?;
        }
        Ok(())
    }

    #[allow(clippy::collapsible_if)]
    async fn run_cleanup_job(&self, job: &MediaJobRow, now: i64) -> Result<(), Error> {
        let payload = match serde_json::from_str::<CleanupPayload>(&job.payload) {
            Ok(payload) => payload,
            Err(_) => {
                // Corrupt payloads burn their remaining attempts for
                // visibility but stop consuming retries early.
                self.media
                    .schedule_retry(&job.id, MAX_JOB_ATTEMPTS + 1, now, "corrupt payload")
                    .await?;
                return Ok(());
            }
        };
        // The finalization survived; only its job deletion was lost. The
        // objects belong to a live record, back off.
        if matches!(payload.mode, CleanupMode::Upload) {
            if let Some(media_id) = payload.media_id {
                match self.media.status_of(&media_id).await {
                    // Ready and published: the objects are load-bearing.
                    Ok(Some(status)) if status == "ready" => {
                        self.media.delete_job(&job.id).await?;
                        return Ok(());
                    }
                    // Ready else absent or in a deletable state: proceed.
                    Ok(_) => {}
                    // Status unreadable (transient DB fault): the row
                    // could be live right now; never delete blind —
                    // leave the job for a later retry.
                    Err(_) => {
                        let attempts = job.attempts + 1;
                        self.media
                            .schedule_retry(
                                &job.id,
                                attempts,
                                now + backoff_secs(attempts),
                                "media status unreadable",
                            )
                            .await?;
                        return Ok(());
                    }
                }
            }
        }
        // Custom connection rows are per-blog, so Blog-mode payloads (the
        // blog row is already gone at run time) always run on their sealed
        // material; Delete/Upload payloads first refresh from the live
        // connection row, so key rotation keeps flowing into pending work.
        // Storage handles the custom snapshot BEFORE the server-location
        // branch: blog payloads for custom connections never resolve
        // against the server bucket simply because they also record its
        // location for bookkeeping. Server payloads carry identity only
        // and get current-config credentials when the bucket still exists.
        let fresh = match payload.connection_id {
            Some(connection_id) if !matches!(payload.mode, CleanupMode::Blog) => {
                self.storage_for_row(
                    &job.owner_id,
                    &payload.blog_id.unwrap_or_default(),
                    Some(connection_id),
                )
                .await
            }
            _ => None,
        };
        let connection = match fresh {
            Some(resolved) => resolved.connection,
            None => match payload.storage {
                // Sealed custom credentials; the payload decided to keep
                // them, so the server bucket is never consulted here.
                Some(material) => match material.open(&self.secrets, &job.id) {
                    Ok(connection) => connection,
                    Err(err) => {
                        self.wait_for_config(job, now, &truncate_detail(&err.to_string()))
                            .await?;
                        return Ok(());
                    }
                },
                None => match &payload.server_location {
                    Some(location) => match self.server_storage.as_ref() {
                        Some(server)
                            if server.connection.account_id == location.account_id
                                && server.connection.bucket == location.bucket =>
                        {
                            server.connection.clone()
                        }
                        _ => {
                            self.wait_for_config(job, now, "server bucket identity changed")
                                .await?;
                            return Ok(());
                        }
                    },
                    None => {
                        self.wait_for_config(job, now, "storage missing in job")
                            .await?;
                        return Ok(());
                    }
                },
            },
        };
        let Ok(store) = self.factory.store_for(&connection) else {
            return Ok(());
        };
        let mut first_error: Option<String> = None;
        for path in &payload.paths {
            if let Err(err) = store.delete(path).await {
                first_error.get_or_insert(err.to_string());
            }
        }
        if let Some(detail) = first_error {
            let attempts = job.attempts + 1;
            self.media
                .schedule_retry(
                    &job.id,
                    attempts,
                    now + backoff_secs(attempts),
                    &truncate_detail(&detail),
                )
                .await?;
            return Ok(());
        }
        // Paths are gone; finish the mode's record bookkeeping.
        match payload.mode {
            CleanupMode::Upload | CleanupMode::Delete => {
                if let Some(media_id) = payload.media_id {
                    self.media.complete_deletion(&media_id, &job.id).await?;
                } else {
                    self.media.delete_job(&job.id).await?;
                }
            }
            CleanupMode::Blog => {
                self.media.delete_job(&job.id).await?;
            }
        }
        Ok(())
    }

    /// Configuration-dependent failures never consume attempts: the job
    /// keeps its due place (`due_jobs` selects on the retry budget only)
    /// and runs again as soon as the configuration it waits for exists.
    /// The wait counter rides in the payload so the backoff still
    /// progresses 30 s → 1 h across consecutive waits — a sparse
    /// maintenance schedule cannot make every wait no longer than 30 s.
    /// Deletion only ever happens with credentials that match the
    /// objects' identity, so waiting beats both guessing keys and
    /// permanently stalling the job — a config restore drains it
    /// without any manual restart step.
    async fn wait_for_config(
        &self,
        job: &MediaJobRow,
        now: i64,
        reason: &str,
    ) -> Result<(), Error> {
        let Ok(mut payload) = serde_json::from_str::<serde_json::Value>(&job.payload) else {
            self.media
                .schedule_retry(&job.id, MAX_JOB_ATTEMPTS + 1, now, "corrupt payload")
                .await?;
            return Ok(());
        };
        let waits = payload
            .get("wait_count")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0)
            + 1;
        payload["wait_count"] = serde_json::Value::from(waits);
        let delay = JOB_BACKOFF_BASE_SECS
            .saturating_mul(1i64 << (waits - 1).clamp(0, 7))
            .min(JOB_BACKOFF_CAP_SECS);
        self.media
            .schedule_wait(&job.id, &payload.to_string(), now + delay, reason)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    /// Registers one durable cleanup job for the paths. The payload
    /// snapshots the sealed connection material of the resolved storage,
    /// so cleanup always targets the exact bucket the objects were
    /// written to — no matter what happens to preferences, connections,
    /// rows or config afterwards. Returns the job id so inline fast paths
    /// can remove it later.
    async fn schedule_cleanup(
        &self,
        owner_id: &Uuid,
        mode: CleanupMode,
        media_id: Uuid,
        blog_id: Uuid,
        storage: &ResolvedStorage,
        paths: Vec<String>,
        due_at: i64,
    ) -> Result<Uuid, Error> {
        let job_id = Uuid::now_v7();
        let material = match storage.connection_id {
            Some(_) => Some(self.job_material(storage, &job_id)?),
            None => None,
        };
        let payload = serde_json::json!({
            "mode": mode,
            "paths": paths,
            "blog_id": blog_id,
            "media_id": media_id,
            "connection_id": storage.connection_id,
            "storage": material,
            "server_location": ServerLocation::of(storage),
        })
        .to_string();
        self.media
            .insert_job(&MediaJobInsert {
                id: job_id,
                owner_id: *owner_id,
                media_id: Some(media_id),
                blog_id: Some(blog_id),
                kind: "cleanup",
                payload,
                next_run_at: due_at,
                created_at: self.clock.now(),
            })
            .await?;
        Ok(job_id)
    }

    /// Durable cleanup job for paths whose storage cannot be resolved at
    /// schedule time: the payload keeps the paths and whatever location
    /// identity the row still holds. The run side treats these payloads
    /// as a configuration wait — never a silent success, never a blind
    /// delete — and a returning config or connection drains the job.
    /// Pure builder: the caller persists it with its own transaction
    /// (the blog cascade) or standalone (single deletes), never both.
    #[allow(clippy::too_many_arguments)]
    fn unresolved_cleanup_job(
        &self,
        owner_id: &Uuid,
        blog_id: Uuid,
        mode: CleanupMode,
        media_id: Option<Uuid>,
        connection_id: Option<Uuid>,
        location: Option<ServerLocation>,
        paths: Vec<String>,
        due_at: i64,
    ) -> PreparedCleanupJob {
        let job_id = Uuid::now_v7();
        PreparedCleanupJob {
            job_id,
            owner_id: *owner_id,
            blog_id,
            payload: serde_json::json!({
                "mode": mode,
                "paths": paths,
                "blog_id": blog_id,
                "media_id": media_id,
                "connection_id": connection_id,
                "storage": serde_json::Value::Null,
                "server_location": location,
            })
            .to_string(),
            due_at,
        }
    }

    /// Sealed cleanup material for one storage reference, AAD-bound to the
    /// given context id (job id for jobs, media id for row snapshots).
    fn job_material(
        &self,
        storage: &ResolvedStorage,
        context: &Uuid,
    ) -> Result<JobStorageMaterial, Error> {
        Ok(JobStorageMaterial {
            account_id: storage.connection.account_id.clone(),
            bucket: storage.connection.bucket.clone(),
            prefix: if storage.connection.prefix.is_empty() {
                None
            } else {
                Some(storage.connection.prefix.clone())
            },
            public_base_url: storage.public_base_url.clone(),
            access_key_id: storage.connection.access_key_id.clone(),
            sealed_secret: self
                .secrets
                .seal(storage.connection.secret_key.as_bytes(), context)?,
        })
    }

    /// Resolves one media row's own storage reference. Custom rows
    /// re-read their (deletion-blocked) connection row — rotation keeps
    /// flowing into old-media cleanup. Server rows re-read the location
    /// snapshot against the *current* server config: same bucket
    /// identity → fresh credentials; a config that moved to another
    /// bucket cannot produce historical credentials, so such rows are
    /// unresolvable and cleanup retry reports it instead of guessing.
    async fn resolve_row_storage(&self, row: &MediaRow) -> Option<ResolvedStorage> {
        if let Some(connection_id) = row.connection_id {
            return self
                .storage_for_row(&row.owner_id, &row.blog_id, Some(connection_id))
                .await;
        }
        let Some(Ok(location)) = row
            .storage_location
            .as_deref()
            .map(serde_json::from_slice::<ServerLocation>)
        else {
            return None;
        };
        let server = self.server_storage.as_ref()?;
        if server.connection.account_id == location.account_id
            && server.connection.bucket == location.bucket
        {
            return Some(ResolvedStorage {
                // Credentials straight from current config, so R2 key
                // rotation never orphans old media cleanup.
                connection: server.connection.clone(),
                public_base_url: row.public_base_url.clone(),
                connection_id: None,
            });
        }
        // Historical bucket identity no longer matches config: never
        // delete objects the operator moved to a different bucket.
        None
    }

    fn validated_connection(
        &self,
        account_id: &str,
        bucket: &str,
        prefix: Option<&str>,
        public_base_url: &str,
        access_key_id: &str,
        secret_key: &str,
    ) -> Result<BucketConnection, Error> {
        let account_id = validate_account_id(account_id)?;
        let bucket = validate_bucket_name(bucket)?;
        let prefix = normalize_prefix(prefix)?;
        validate_public_base_url(public_base_url)?;
        if access_key_id.is_empty()
            || access_key_id.len() > MAX_CREDENTIAL_BYTES
            || secret_key.is_empty()
            || secret_key.len() > MAX_CREDENTIAL_BYTES
        {
            return Err(Error::InvalidInput);
        }
        Ok(BucketConnection {
            account_id,
            bucket,
            prefix: prefix.unwrap_or_default(),
            access_key_id: access_key_id.to_owned(),
            secret_key: secret_key.to_owned(),
        })
    }
}

/// `ResolvedStorage` notes where one media upload must go: the concrete
/// bucket plus the base that publishes its objects.
pub(crate) struct ResolvedStorage {
    pub connection: BucketConnection,
    pub public_base_url: String,
    pub connection_id: Option<Uuid>,
}

/// Cleanup payload modes: each snapshots what execution must finish once
/// the paths are gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum CleanupMode {
    /// Dying upload or failed finalize: delete anything already written,
    /// finish by removing the media row.
    Upload,
    /// Explicit delete request: delete paths, finish by removing the row.
    Delete,
    /// Blog cascade: paths only; the rows are already gone.
    Blog,
}

/// Job payload: the object paths plus the exact sealed storage reference
/// used when the objects were written.
#[derive(Debug, serde::Deserialize)]
struct CleanupPayload {
    mode: CleanupMode,
    paths: Vec<String>,
    blog_id: Option<Uuid>,
    media_id: Option<Uuid>,
    /// Custom connections carry their row reference, so pending jobs can
    /// adopt rotated credentials instead of the frozen snapshot.
    connection_id: Option<Uuid>,
    storage: Option<JobStorageMaterial>,
    /// Server-bucket jobs travel by location identity; credentials never
    /// enter job payloads or the database.
    server_location: Option<ServerLocation>,
}

/// Location identity of one server-bucket row: pins the bucket without
/// ever copying server secrets into the database; credentials always
/// resolve from the current configuration when the identity still exists.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct ServerLocation {
    account_id: String,
    bucket: String,
    prefix: Option<String>,
    public_base_url: String,
}

impl ServerLocation {
    /// Snapshot of one upload's resolving storage.
    fn of(storage: &ResolvedStorage) -> ServerLocation {
        ServerLocation {
            account_id: storage.connection.account_id.clone(),
            bucket: storage.connection.bucket.clone(),
            prefix: if storage.connection.prefix.is_empty() {
                None
            } else {
                Some(storage.connection.prefix.clone())
            },
            public_base_url: storage.public_base_url.clone(),
        }
    }
}

/// Sealed connection material for one cleanup job.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct JobStorageMaterial {
    account_id: String,
    bucket: String,
    prefix: Option<String>,
    public_base_url: String,
    access_key_id: String,
    /// AES-256-GCM blob for the R2 secret, AAD-bound to the job id.
    sealed_secret: Vec<u8>,
}

impl JobStorageMaterial {
    fn open(&self, secrets: &SecretCipher, job_id: &Uuid) -> Result<BucketConnection, Error> {
        let secret = secrets.open(&self.sealed_secret, job_id)?;
        let secret = String::from_utf8(secret).map_err(|e| internal_io(&e.to_string()))?;
        Ok(BucketConnection {
            account_id: self.account_id.clone(),
            bucket: self.bucket.clone(),
            prefix: self.prefix.clone().unwrap_or_default(),
            access_key_id: self.access_key_id.clone(),
            secret_key: secret,
        })
    }
}

/// A job fully resolved while the rows still existed, waiting for the code
/// seal that commits it in the same transaction as the blog deletion.
pub struct PreparedCleanupJob {
    job_id: Uuid,
    owner_id: Uuid,
    blog_id: Uuid,
    payload: String,
    due_at: i64,
}

impl PreparedCleanupJob {
    /// insert-shape for the transactional commit on the blog repo.
    pub fn to_insert(&self) -> MediaJobInsert {
        MediaJobInsert {
            id: self.job_id,
            owner_id: self.owner_id,
            media_id: None,
            blog_id: Some(self.blog_id),
            kind: "cleanup",
            payload: self.payload.clone(),
            next_run_at: self.due_at,
            created_at: self.due_at.saturating_sub(1),
        }
    }
}

/// Variant rows committed with the ready transition: exact byte counts and
/// dimensions of everything that was encoded. Key candidates without a
/// variant (source narrower than the target width) never become rows.
fn variant_inserts_for(
    id: Uuid,
    processed: &Processed,
    source_key: &str,
    variant_keys: &[(String, u32)],
) -> Vec<MediaVariantInsert> {
    let mut inserts = vec![MediaVariantInsert {
        media_id: id,
        label: "source".to_owned(),
        object_key: source_key.to_owned(),
        mime: processed.source.mime.to_owned(),
        width: Some(processed.width),
        height: Some(processed.height),
        bytes: len64(processed.source.bytes.len()),
    }];
    for variant in &processed.variants {
        if let Some((key, _)) = variant_keys
            .iter()
            .find(|(_, width)| *width == variant.width)
        {
            inserts.push(MediaVariantInsert {
                media_id: id,
                label: variant.label.clone(),
                object_key: key.clone(),
                mime: variant.encoded.mime.to_owned(),
                width: Some(variant.width),
                height: Some(variant.height),
                bytes: len64(variant.encoded.bytes.len()),
            });
        }
    }
    inserts
}

/// Wire shape of one media record as the management API returns it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MediaResponse {
    pub id: Uuid,
    pub blog_id: Uuid,
    pub status: String,
    pub mime: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub stored_bytes: i64,
    pub created_at: i64,
    pub public_url: Option<String>,
    pub variants: Vec<MediaVariantResponse>,
}

/// Wire shape of one generated variant.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MediaVariantResponse {
    pub label: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub bytes: i64,
    pub public_url: Option<String>,
}

/// Response for one stored row: the `source` variant is the canonical URL
/// and every other row lists its responsive alternative. The publishing
/// base is the row's own commit-time snapshot.
fn media_response(row: &MediaRow, variants: &[MediaVariantRow]) -> MediaResponse {
    let url_for = |key: &str| crate::storage::format_public_url(&row.public_base_url, key);
    let source = variants.iter().find(|variant| variant.label == "source");
    MediaResponse {
        id: row.id,
        blog_id: row.blog_id,
        status: row.status.clone(),
        mime: row.mime.clone(),
        width: row.width,
        height: row.height,
        stored_bytes: row.stored_bytes,
        created_at: row.created_at,
        public_url: source.and_then(|variant| url_for(&variant.object_key)),
        variants: variants
            .iter()
            .filter(|variant| variant.label != "source")
            .map(|variant| MediaVariantResponse {
                label: variant.label.clone(),
                width: variant.width,
                height: variant.height,
                bytes: variant.bytes,
                public_url: url_for(&variant.object_key),
            })
            .collect(),
    }
}

/// Public delivery metadata for one referenced media object.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MediaPublicMeta {
    pub id: Uuid,
    pub source_url: String,
    pub source_width: Option<u32>,
    pub source_height: Option<u32>,
    pub mime: String,
    /// Largest available drawable surface for in-page inspection: the
    /// widest responsive variant, or the source when none exists.
    pub zoom_url: String,
    pub variants: Vec<MediaPublicVariantMeta>,
}

/// One responsive variant in public delivery metadata.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MediaPublicVariantMeta {
    pub label: String,
    pub url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

fn internal_io(message: &str) -> Error {
    Error::Internal(InternalError::new(std::io::Error::other(
        message.to_owned(),
    )))
}

/// Error details carried in job failures: informative, still bounded.
fn truncate_detail(detail: &str) -> String {
    use std::fmt::Write;
    let mut bounded = String::new();
    let mut bytes = 0;
    for character in detail.chars() {
        let char_len = character.len_utf8();
        if bytes + char_len > 128 {
            let _ = bounded.write_char('…');
            break;
        }
        let _ = bounded.write_char(character);
        bytes += char_len;
    }
    bounded
}

/// Backoff schedule: 30s doubling up to one hour, one extra attempt past
/// `MAX_JOB_ATTEMPTS` is never scheduled.
fn backoff_secs(attempts: i64) -> i64 {
    let exponent = (attempts - 1).clamp(0, 6);
    JOB_BACKOFF_BASE_SECS
        .saturating_mul(1i64 << exponent)
        .min(JOB_BACKOFF_CAP_SECS)
}

fn len64(bytes: usize) -> i64 {
    i64::try_from(bytes).unwrap_or(i64::MAX)
}

fn valid_name(name: Option<&str>) -> Result<Option<String>, Error> {
    let Some(name) = name.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if name.len() > MAX_NAME_BYTES
        || name
            .chars()
            .any(|ch| !(0x20..=0x7E).contains(&(ch as u32)) && (ch as u32) < 0xA0)
    {
        return Err(Error::InvalidInput);
    }
    Ok(Some(name.to_owned()))
}

fn insert_of(row: &StorageConnectionRow) -> StorageConnectionInsert {
    StorageConnectionInsert {
        id: row.id,
        blog_id: row.blog_id,
        owner_id: row.owner_id,
        name: row.name.clone(),
        account_id: row.account_id.clone(),
        bucket: row.bucket.clone(),
        prefix: row.prefix.clone(),
        public_base_url: row.public_base_url.clone(),
        access_key_id: row.access_key_id.clone(),
        sealed_secret: row.secret_key.clone(),
        verified_at: row.verified_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}
