pub mod account;
pub mod catalog;
pub mod curseforge;
pub mod error;
pub mod java;
pub mod launcher;
pub mod modpack;
pub mod modrinth;
pub mod mojang;
pub mod paths;
pub mod profile;
pub mod updater;

pub use account::{Account, AccountRole, AccountStore, DeviceCodeResponse, MsaAuthClient};
pub use catalog::{
    CatalogClient, CatalogDependency, CatalogDetail, CatalogFile, CatalogGalleryItem, CatalogHit,
    CatalogSearchResponse, CatalogSort, CatalogVersion, ContentKind, ContentSource,
    ContentUpdateInfo, InstalledContentItem, InstalledContentMeta, SourceFilter, WorldInfo,
    list_installed_content, list_saved_worlds, remove_installed_content, sync_content_to_profiles,
    toggle_installed_content,
};
pub use curseforge::{
    CLASS_DATAPACKS, CLASS_MODPACKS, CLASS_MODS, CLASS_RESOURCEPACKS, CLASS_SHADERS, CfAsset,
    CfAuthor, CfCategory, CfDependency, CfFile, CfFileIndex, CfFingerprintMatch,
    CfFingerprintResult, CfHash, CfLinks, CfMod, CfPagination, CfSearchResponse, CurseForgeClient,
    MC_GAME_ID, compute_murmur2_fingerprint,
};
pub use error::{MubError, Result};
pub use java::{
    JavaInstallation, detect_java_installations, pick_best_java, required_java_major,
    resolve_java_for_mc,
};
pub use launcher::{InstanceManager, InstanceState, InstanceStatus};
pub use modpack::{
    ModpackImportResult, MrpackEnv, MrpackFile, MrpackIndex, RestrictedPackFile, export_mrpack,
    import_curseforge_pack, import_mrpack, install_modpack_from_catalog,
};
pub use modrinth::{
    InstalledMod, ModrinthClient, ModrinthEnvironment, ModrinthGalleryItem, ModrinthHit,
    ModrinthLicense, ModrinthProject, ModrinthSearchResponse, ModrinthSort, ModrinthVersion,
    list_installed_mods, list_installed_project_ids, remove_installed_mod, toggle_installed_mod,
};
pub use mojang::{LoaderVersionInfo, PreparedVersion, VersionManager};
pub use paths::{AppPaths, open_in_file_manager};
pub use profile::{
    LauncherSettings, ModLoader, Profile, ProfileKind, ProfileStore, SUPPORTED_THEME_PALETTES,
};
pub use updater::{
    CURRENT_VERSION as APP_VERSION, InstalledUpdateResult, UpdateCheckResult, check_for_updates,
    cleanup_old_update_files, download_and_install_update, is_newer_version,
    spawn_updated_relaunch,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_roundtrip_and_paths() {
        let dir = std::env::temp_dir().join(format!("mub_test_{}", uuid::Uuid::new_v4()));
        let paths = AppPaths::from_root(&dir);
        paths.ensure_dirs().unwrap();

        let mut acc_store = AccountStore::default();
        let bot = Account::new_offline("Bot01", AccountRole::Bot);
        let bot_id = bot.id.clone();
        acc_store.upsert(bot);
        acc_store.save(&paths.accounts_file()).unwrap();

        let loaded_acc = AccountStore::load(&paths.accounts_file()).unwrap();
        assert_eq!(loaded_acc.accounts.len(), 1);
        assert_eq!(loaded_acc.get(&bot_id).unwrap().username, "Bot01");

        let mut prof_store = ProfileStore::default();
        let mut prof = Profile::new(
            "FarmBot1",
            ProfileKind::Bot,
            "1.21.4",
            ModLoader::Fabric {
                loader_version: "0.16.10".to_string(),
            },
        );
        prof.account_id = Some(bot_id);
        let prof_id = prof.id.clone();
        prof_store.upsert(prof);
        prof_store.save(&paths.profiles_file()).unwrap();

        let loaded_prof = ProfileStore::load(&paths.profiles_file()).unwrap();
        assert_eq!(loaded_prof.profiles.len(), 1);
        assert_eq!(loaded_prof.get(&prof_id).unwrap().name, "FarmBot1");

        // Verify mods_dir listing & removal
        std::fs::create_dir_all(paths.mods_dir(&prof_id)).unwrap();
        std::fs::write(paths.mods_dir(&prof_id).join("example-mod.jar"), b"jar").unwrap();
        let mods = list_installed_mods(&paths, &prof_id).unwrap();
        assert_eq!(mods.len(), 1);
        assert_eq!(mods[0].filename, "example-mod.jar");
        remove_installed_mod(&paths, &prof_id, "example-mod.jar").unwrap();
        assert!(list_installed_mods(&paths, &prof_id).unwrap().is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn settings_and_profiles_backwards_compat_and_roundtrip() {
        let dir = std::env::temp_dir().join(format!("mub_compat_{}", uuid::Uuid::new_v4()));
        let paths = AppPaths::from_root(&dir);
        paths.ensure_dirs().unwrap();

        // Legacy settings.json without theme_palette or curseforge_api_key
        let legacy_settings_json = r#"{
            "default_java_path": "java",
            "msa_client_id": "00000000402b5328",
            "batch_launch_stagger_ms": 1200,
            "max_concurrent_downloads": 8
        }"#;
        std::fs::write(paths.settings_file(), legacy_settings_json).unwrap();
        let mut loaded_settings = LauncherSettings::load(&paths.settings_file()).unwrap();
        assert_eq!(loaded_settings.theme_palette, "synth_terminal");
        assert_eq!(loaded_settings.curseforge_api_key, "");
        assert_eq!(loaded_settings.batch_launch_stagger_ms, 1200);

        loaded_settings.theme_palette = "tokyo_night".to_string();
        loaded_settings.curseforge_api_key = "  my-cf-key  ".to_string();
        assert_eq!(
            loaded_settings.effective_curseforge_api_key().as_deref(),
            Some("my-cf-key")
        );
        loaded_settings.save(&paths.settings_file()).unwrap();
        let reloaded_settings = LauncherSettings::load(&paths.settings_file()).unwrap();
        assert_eq!(reloaded_settings.theme_palette, "tokyo_night");

        // Legacy profiles.json without group, playtime_secs, source_pack
        let legacy_profiles_json = r#"{
            "profiles": [{
                "id": "legacy-1",
                "name": "OldPlayer",
                "kind": "player",
                "mc_version": "1.20.4",
                "mod_loader": { "type": "fabric", "loader_version": "0.15.11" },
                "account_id": null,
                "java_path": null,
                "memory_min_mb": 512,
                "memory_max_mb": 4096,
                "jvm_args": [],
                "quick_connect": null,
                "resolution": null,
                "tags": [],
                "created_at": "2024-06-01T00:00:00Z",
                "last_launched_at": null
            }]
        }"#;
        std::fs::write(paths.profiles_file(), legacy_profiles_json).unwrap();
        let mut store = ProfileStore::load(&paths.profiles_file()).unwrap();
        assert_eq!(store.profiles.len(), 1);
        assert_eq!(store.profiles[0].group, None);
        assert_eq!(store.profiles[0].playtime_secs, 0);
        assert_eq!(store.profiles[0].source_pack, None);

        let mut cloned = store.profiles[0].clone_with_name("QuiltClone");
        cloned.mod_loader = ModLoader::Quilt {
            loader_version: "0.26.3".to_string(),
        };
        cloned.group = Some("builders".to_string());
        cloned.source_pack = Some("MyPack".to_string());
        assert_eq!(cloned.mod_loader.slug(), Some("quilt"));
        assert_eq!(cloned.mod_loader.curseforge_loader_id(), Some(5));
        assert_eq!(
            ModLoader::Forge {
                loader_version: "47.2.0".to_string()
            }
            .curseforge_loader_id(),
            Some(1)
        );
        store.upsert(cloned);
        store.save(&paths.profiles_file()).unwrap();

        let reloaded = ProfileStore::load(&paths.profiles_file()).unwrap();
        assert_eq!(reloaded.profiles.len(), 2);
        assert_eq!(
            reloaded.find_by_name("QuiltClone").unwrap().group.as_deref(),
            Some("builders")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn java_version_requirements_and_selection() {
        assert_eq!(required_java_major("1.21.4"), 21);
        assert_eq!(required_java_major("1.21"), 21);
        assert_eq!(required_java_major("1.20.6"), 21);
        assert_eq!(required_java_major("1.20.5"), 21);
        assert_eq!(required_java_major("1.20.4"), 17);
        assert_eq!(required_java_major("1.20.1"), 17);
        assert_eq!(required_java_major("1.19.2"), 17);
        assert_eq!(required_java_major("1.18.2"), 17);
        assert_eq!(required_java_major("1.17.1"), 16);
        assert_eq!(required_java_major("1.16.5"), 8);
        assert_eq!(required_java_major("1.12.2"), 8);
        assert_eq!(required_java_major("24w14a"), 21);

        let installs = vec![
            JavaInstallation {
                path: std::path::PathBuf::from("/jdk8/bin/java"),
                version: "1.8.0_411".to_string(),
                major: 8,
                vendor: "Temurin".to_string(),
            },
            JavaInstallation {
                path: std::path::PathBuf::from("/jdk17/bin/java"),
                version: "17.0.12".to_string(),
                major: 17,
                vendor: "Temurin".to_string(),
            },
            JavaInstallation {
                path: std::path::PathBuf::from("/jdk21/bin/java"),
                version: "21.0.5".to_string(),
                major: 21,
                vendor: "Temurin".to_string(),
            },
        ];
        assert_eq!(pick_best_java(&installs, "1.21.4").unwrap().major, 21);
        assert_eq!(pick_best_java(&installs, "1.20.1").unwrap().major, 17);
        assert_eq!(pick_best_java(&installs, "1.17.1").unwrap().major, 17);
        assert_eq!(pick_best_java(&installs, "1.16.5").unwrap().major, 8);
    }

    #[test]
    fn installed_content_toggle_remove_and_sync() {
        let dir = std::env::temp_dir().join(format!("mub_content_{}", uuid::Uuid::new_v4()));
        let paths = AppPaths::from_root(&dir);
        paths.ensure_dirs().unwrap();

        let p1 = Profile::new(
            "Source",
            ProfileKind::Player,
            "1.21.4",
            ModLoader::Fabric {
                loader_version: "0.16.10".to_string(),
            },
        );
        let p2 = Profile::new(
            "Target",
            ProfileKind::Bot,
            "1.21.4",
            ModLoader::Fabric {
                loader_version: "0.16.10".to_string(),
            },
        );

        std::fs::create_dir_all(paths.mods_dir(&p1.id)).unwrap();
        std::fs::write(paths.mods_dir(&p1.id).join("sodium.jar"), b"fake-jar").unwrap();
        catalog::upsert_content_meta(
            &paths,
            &p1.id,
            InstalledContentMeta {
                source: ContentSource::Modrinth,
                kind: ContentKind::Mod,
                project_id: "AANobbMI".to_string(),
                version_id: "v1".to_string(),
                version_number: "0.6.0".to_string(),
                filename: "sodium.jar".to_string(),
                sha1: Some("deadbeef".to_string()),
                murmur2: Some(12345),
                download_url: None,
                title: Some("Sodium".to_string()),
                icon_url: None,
            },
        )
        .unwrap();

        let list = list_installed_content(&paths, &p1.id, ContentKind::Mod).unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].enabled);
        assert_eq!(list[0].meta.as_ref().unwrap().project_id, "AANobbMI");

        toggle_installed_content(&paths, &p1.id, ContentKind::Mod, "sodium.jar").unwrap();
        let list = list_installed_content(&paths, &p1.id, ContentKind::Mod).unwrap();
        assert_eq!(list.len(), 1);
        assert!(!list[0].enabled);
        assert_eq!(list[0].filename, "sodium.jar.disabled");
        assert!(list[0].meta.is_some());

        // Sync to p2
        let copied = sync_content_to_profiles(
            &paths,
            &p1.id,
            std::slice::from_ref(&p2.id),
            ContentKind::Mod,
        )
        .unwrap();
        assert_eq!(copied, 1);
        let p2_list = list_installed_content(&paths, &p2.id, ContentKind::Mod).unwrap();
        assert_eq!(p2_list.len(), 1);
        assert!(p2_list[0].meta.is_some());

        // Remove cleans disk + manifest
        remove_installed_content(&paths, &p1.id, ContentKind::Mod, "sodium.jar.disabled").unwrap();
        assert!(
            list_installed_content(&paths, &p1.id, ContentKind::Mod)
                .unwrap()
                .is_empty()
        );
        let manifest_raw =
            std::fs::read_to_string(catalog::content_manifest_path(&paths, &p1.id)).unwrap();
        assert!(!manifest_raw.contains("AANobbMI"));

        // Worlds listing
        let world_dp = paths
            .saves_dir(&p1.id)
            .join("SurvivalWorld")
            .join("datapacks");
        std::fs::create_dir_all(&world_dp).unwrap();
        std::fs::write(world_dp.join("terralith.zip"), b"zip").unwrap();
        let worlds = list_saved_worlds(&paths, &p1.id).unwrap();
        assert_eq!(worlds.len(), 1);
        assert_eq!(worlds[0].folder_name, "SurvivalWorld");
        assert_eq!(worlds[0].datapack_count, 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn mrpack_export_and_import_roundtrip() {
        let dir = std::env::temp_dir().join(format!("mub_mrpack_{}", uuid::Uuid::new_v4()));
        let paths = AppPaths::from_root(&dir);
        paths.ensure_dirs().unwrap();

        let src_prof = Profile::new(
            "PackSource",
            ProfileKind::Player,
            "1.21.1",
            ModLoader::NeoForge {
                loader_version: "21.1.72".to_string(),
            },
        );
        let config_dir = paths.config_dir(&src_prof.id);
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("sample.toml"), b"key = true\n").unwrap();
        std::fs::write(
            paths.instance_dir(&src_prof.id).join("options.txt"),
            b"fov:1.0\n",
        )
        .unwrap();

        let mrpack_path = dir.join("exported.mrpack");
        export_mrpack(
            &paths,
            &src_prof,
            &mrpack_path,
            "1.0.0",
            Some("Test pack summary"),
        )
        .unwrap();
        assert!(mrpack_path.exists());

        let imported = import_mrpack(
            &paths,
            &mrpack_path,
            Some("ImportedPack"),
            ProfileKind::Bot,
            4,
            |_| {},
        )
        .await
        .unwrap();

        assert_eq!(imported.profile.name, "ImportedPack");
        assert_eq!(imported.profile.mc_version, "1.21.1");
        assert_eq!(
            imported.profile.mod_loader,
            ModLoader::NeoForge {
                loader_version: "21.1.72".to_string()
            }
        );
        assert_eq!(imported.profile.source_pack.as_deref(), Some("PackSource"));
        assert_eq!(imported.extracted_overrides, 2);
        assert_eq!(
            std::fs::read_to_string(
                paths.config_dir(&imported.profile.id).join("sample.toml")
            )
            .unwrap(),
            "key = true\n"
        );
        assert_eq!(
            std::fs::read_to_string(
                paths
                    .instance_dir(&imported.profile.id)
                    .join("options.txt")
            )
            .unwrap(),
            "fov:1.0\n"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
