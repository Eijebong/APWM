use crate::{IndexLock, World};
use reqwest::Url;
    index_lock: &IndexLock,
    lobby_url: &Option<Url>,
    let diff = diff_world(from, to, ap_index_url, ap_index_ref, index_lock, lobby_url).await?;
    index_lock: &IndexLock,
    lobby_url: &Option<Url>,
                    index_lock,
                    lobby_url,
                    index_lock,
                    lobby_url,
    index_lock: &IndexLock,
    lobby_url: &Option<Url>,
                index_lock,
                lobby_url,
            .extract_to(
                to_version,
                to_tmpdir.path(),
                ap_index_url,
                ap_index_ref,
                index_lock,
                lobby_url,
            )
    use crate::{IndexLock, World, WorldOrigin};
        let index_lock = IndexLock::default();
        let diff = diff_world(None, Some(&new_world), "", "", &index_lock, &None).await?;
        let index_lock = IndexLock::default();
        let diff = diff_world(Some(&old_world), None, "", "", &index_lock, &None).await?;
        let index_lock = IndexLock::default();
        let diff = diff_world(
            Some(&old_world),
            Some(&new_world),
            "",
            "",
            &index_lock,
            &None,
        )
        .await?;