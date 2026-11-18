pub mod account;
pub mod error;
pub mod launcher;
pub mod modrinth;
pub mod mojang;
pub mod paths;
pub mod profile;

pub use account::{Account, AccountRole, AccountStore, DeviceCodeResponse, MsaAuthClient};
pub use error::{MubError, Result};
pub use launcher::{InstanceManager, InstanceState, InstanceStatus};
pub use modrinth::{
    InstalledMod, ModrinthClient, ModrinthEnvironment, ModrinthGalleryItem, ModrinthHit,
    ModrinthProject, ModrinthSearchResponse, ModrinthSort, ModrinthVersion, list_installed_mods,
    list_installed_project_ids, remove_installed_mod, toggle_installed_mod,
};
pub use mojang::VersionManager;
pub use paths::AppPaths;
pub use profile::{LauncherSettings, ModLoader, Profile, ProfileKind, ProfileStore};

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
}
