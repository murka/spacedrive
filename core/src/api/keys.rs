use std::io::{Read, Write};
use std::{path::PathBuf, str::FromStr};

use sd_crypto::keys::keymanager::StoredKey;
use sd_crypto::{
	crypto::stream::Algorithm,
	keys::{hashing::HashingAlgorithm, keymanager::KeyManager},
	Protected,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

use crate::util::db::write_storedkey_to_db;
use crate::{invalidate_query, prisma::key};

use super::{utils::LibraryRequest, RouterBuilder};

#[derive(Type, Deserialize)]
pub struct KeyAddArgs {
	algorithm: Algorithm,
	hashing_algorithm: HashingAlgorithm,
	key: String,
	library_sync: bool,
	automount: bool,
}

#[derive(Type, Deserialize)]
pub struct KeyNameUpdateArgs {
	uuid: Uuid,
	name: String,
}

#[derive(Type, Deserialize)]
pub struct SetMasterPasswordArgs {
	password: String,
	secret_key: String,
}

#[derive(Type, Deserialize)]
pub struct RestoreBackupArgs {
	password: String,
	secret_key: String,
	path: PathBuf,
}

#[derive(Type, Deserialize)]
pub struct OnboardingArgs {
	algorithm: Algorithm,
	hashing_algorithm: HashingAlgorithm,
}

#[derive(Type, Deserialize)]
pub struct MasterPasswordChangeArgs {
	password: String,
	algorithm: Algorithm,
	hashing_algorithm: HashingAlgorithm,
}

#[derive(Type, Serialize)]
pub struct OnboardingKeys {
	master_password: String,
	secret_key: String,
}

#[derive(Type, Deserialize)]
pub struct AutomountUpdateArgs {
	uuid: Uuid,
	status: bool,
}

pub(crate) fn mount() -> RouterBuilder {
	RouterBuilder::new()
		.library_query("list", |t| {
			t(|_, _: (), library| async move { Ok(library.key_manager.dump_keystore()) })
		})
		// do not unlock the key manager until this route returns true
		.library_query("hasMasterPassword", |t| {
			t(|_, _: (), library| async move { Ok(library.key_manager.has_master_password()?) })
		})
		// this is so we can show the key as mounted in the UI
		.library_query("listMounted", |t| {
			t(|_, _: (), library| async move { Ok(library.key_manager.get_mounted_uuids()) })
		})
		.library_query("getKey", |t| {
			t(|_, key_uuid: Uuid, library| async move {
				let key = library.key_manager.get_key(key_uuid)?;

				let key_string = String::from_utf8(key.expose().clone()).map_err(|_| {
					rspc::Error::new(
						rspc::ErrorCode::InternalServerError,
						"Error serializing bytes to String".into(),
					)
				})?;

				Ok(key_string)
			})
		})
		.library_mutation("mount", |t| {
			t(|_, key_uuid: Uuid, library| async move {
				library.key_manager.mount(key_uuid)?;
				// we also need to dispatch jobs that automatically decrypt preview media and metadata here
				invalidate_query!(library, "keys.listMounted");
				Ok(())
			})
		})
		.library_mutation("updateKeyName", |t| {
			t(|_, args: KeyNameUpdateArgs, library| async move {
				library
					.db
					.key()
					.update(
						key::uuid::equals(args.uuid.to_string()),
						vec![key::SetParam::SetName(Some(args.name))],
					)
					.exec()
					.await?;

				Ok(())
			})
		})
		.library_mutation("unmount", |t| {
			t(|_, key_uuid: Uuid, library| async move {
				library.key_manager.unmount(key_uuid)?;
				// we also need to delete all in-memory decrypted data associated with this key
				invalidate_query!(library, "keys.listMounted");
				Ok(())
			})
		})
		.library_mutation("clearMasterPassword", |t| {
			t(|_, _: (), library| async move {
				// This technically clears the root key, but it means the same thing to the frontend
				library.key_manager.clear_root_key()?;

				invalidate_query!(library, "keys.hasMasterPassword");
				Ok(())
			})
		})
		.library_mutation("syncKeyToLibrary", |t| {
			t(|_, key_uuid: Uuid, library| async move {
				let key = library.key_manager.save_to_database(key_uuid)?;

				// does not check that the key doesn't exist before writing
				write_storedkey_to_db(library.db.clone(), &key).await?;

				invalidate_query!(library, "keys.list");
				Ok(())
			})
		})
		.library_mutation("updateAutomountStatus", |t| {
			t(|_, args: AutomountUpdateArgs, library| async move {
				if !library.key_manager.is_memory_only(args.uuid)? {
					library
						.key_manager
						.change_automount_status(args.uuid, args.status)?;

					library
						.db
						.key()
						.update(
							key::uuid::equals(args.uuid.to_string()),
							vec![key::SetParam::SetAutomount(args.status)],
						)
						.exec()
						.await?;

					invalidate_query!(library, "keys.list");
				}

				Ok(())
			})
		})
		.library_mutation("deleteFromLibrary", |t| {
			t(|_, key_uuid: Uuid, library| async move {
				if !library.key_manager.is_memory_only(key_uuid)? {
					library
						.db
						.key()
						.delete(key::uuid::equals(key_uuid.to_string()))
						.exec()
						.await?;
				}

				library.key_manager.remove_key(key_uuid)?;

				// we also need to delete all in-memory decrypted data associated with this key
				invalidate_query!(library, "keys.list");
				invalidate_query!(library, "keys.listMounted");
				invalidate_query!(library, "keys.getDefault");
				Ok(())
			})
		})
		.library_mutation("onboarding", |t| {
			t(|_, args: OnboardingArgs, library| async move {
				let bundle = KeyManager::onboarding(args.algorithm, args.hashing_algorithm)?;

				let verification_key = bundle.verification_key;

				// remove old nil-id keys if they were set
				// they possibly won't be, but we CANNOT have multiple
				library
					.db
					.key()
					.delete_many(vec![key::uuid::equals(Uuid::nil().to_string())])
					.exec()
					.await?;

				write_storedkey_to_db(library.db.clone(), &verification_key).await?;

				let keys = OnboardingKeys {
					master_password: bundle.master_password.expose().clone(),
					secret_key: base64::encode(bundle.secret_key.expose()),
				};

				Ok(keys)
			})
		})
		.library_mutation("setMasterPassword", |t| {
			t(|_, args: SetMasterPasswordArgs, library| async move {
				// if this returns an error, the user MUST re-enter the correct password
				library.key_manager.set_master_password(
					Protected::new(args.password),
					Protected::new(args.secret_key),
				)?;

				let automount = library
					.db
					.key()
					.find_many(vec![key::automount::equals(true)])
					.exec()
					.await?;

				for key in automount {
					library
						.key_manager
						.mount(Uuid::from_str(&key.uuid).map_err(|_| {
							rspc::Error::new(
								rspc::ErrorCode::InternalServerError,
								"Error deserializing UUID from string".into(),
							)
						})?)?;
				}

				invalidate_query!(library, "keys.hasMasterPassword");

				Ok(())
			})
		})
		.library_mutation("setDefault", |t| {
			t(|_, key_uuid: Uuid, library| async move {
				library.key_manager.set_default(key_uuid)?;

				library
					.db
					.key()
					.update_many(
						vec![key::default::equals(true)],
						vec![key::SetParam::SetDefault(false)],
					)
					.exec()
					.await?;
				library
					.db
					.key()
					.update(
						key::uuid::equals(key_uuid.to_string()),
						vec![key::SetParam::SetDefault(true)],
					)
					.exec()
					.await?;

				invalidate_query!(library, "keys.getDefault");
				Ok(())
			})
		})
		.library_query("getDefault", |t| {
			t(|_, _: (), library| async move {
				let default = library.key_manager.get_default();

				if let Ok(default_key) = default {
					Ok(Some(default_key))
				} else {
					Ok(None)
				}
			})
		})
		.library_mutation("unmountAll", |t| {
			t(|_, _: (), library| async move {
				library.key_manager.empty_keymount();
				invalidate_query!(library, "keys.listMounted");
				Ok(())
			})
		})
		// this also mounts the key
		.library_mutation("add", |t| {
			t(|_, args: KeyAddArgs, library| async move {
				// register the key with the keymanager
				let uuid = library.key_manager.add_to_keystore(
					Protected::new(args.key.as_bytes().to_vec()),
					args.algorithm,
					args.hashing_algorithm,
					!args.library_sync,
					args.automount,
					None,
				)?;

				let stored_key = library.key_manager.access_keystore(uuid)?;

				if args.library_sync {
					write_storedkey_to_db(library.db.clone(), &stored_key).await?;

					if args.automount {
						library
							.db
							.key()
							.update(
								key::uuid::equals(uuid.to_string()),
								vec![key::SetParam::SetAutomount(true)],
							)
							.exec()
							.await?;
					}
				}

				// mount the key
				library.key_manager.mount(uuid)?;

				invalidate_query!(library, "keys.list");
				invalidate_query!(library, "keys.listMounted");
				Ok(())
			})
		})
		.library_mutation("backupKeystore", |t| {
			t(|_, path: PathBuf, library| async move {
				// dump all stored keys that are in the key manager (maybe these should be taken from prisma as this will include even "non-sync with library" keys)
				let mut stored_keys = library.key_manager.dump_keystore();
				// include the verification key at the time of backup
				stored_keys.push(library.key_manager.get_verification_key()?);
				stored_keys.retain(|k| !k.memory_only);

				let mut output_file = std::fs::File::create(path).map_err(|_| {
					rspc::Error::new(
						rspc::ErrorCode::InternalServerError,
						"Error creating file".into(),
					)
				})?;
				output_file
					.write_all(&serde_json::to_vec(&stored_keys).map_err(|_| {
						rspc::Error::new(
							rspc::ErrorCode::InternalServerError,
							"Error serializing keystore".into(),
						)
					})?)
					.map_err(|_| {
						rspc::Error::new(
							rspc::ErrorCode::InternalServerError,
							"Error writing key backup to file".into(),
						)
					})?;
				Ok(())
			})
		})
		.library_mutation("restoreKeystore", |t| {
			t(|_, args: RestoreBackupArgs, library| async move {
				let mut input_file = std::fs::File::open(args.path).map_err(|_| {
					rspc::Error::new(
						rspc::ErrorCode::InternalServerError,
						"Error opening backup file".into(),
					)
				})?;

				let mut backup = Vec::new();

				input_file.read_to_end(&mut backup).map_err(|_| {
					rspc::Error::new(
						rspc::ErrorCode::InternalServerError,
						"Error reading backup file".into(),
					)
				})?;

				let stored_keys: Vec<StoredKey> =
					serde_json::from_slice(&backup).map_err(|_| {
						rspc::Error::new(
							rspc::ErrorCode::InternalServerError,
							"Error deserializing backup".into(),
						)
					})?;

				let updated_keys = library.key_manager.import_keystore_backup(
					Protected::new(args.password),
					Protected::new(args.secret_key),
					&stored_keys,
				)?;

				for key in &updated_keys {
					write_storedkey_to_db(library.db.clone(), key).await?;
				}

				invalidate_query!(library, "keys.list");
				invalidate_query!(library, "keys.listMounted");

				Ok(updated_keys.len())
			})
		})
		.library_mutation("changeMasterPassword", |t| {
			t(|_, args: MasterPasswordChangeArgs, library| async move {
				let bundle = library.key_manager.change_master_password(
					Protected::new(args.password),
					args.algorithm,
					args.hashing_algorithm,
				)?;

				// remove old nil-id keys if they were set
				library
					.db
					.key()
					.delete_many(vec![key::uuid::equals(Uuid::nil().to_string())])
					.exec()
					.await?;

				// write the new verification key
				write_storedkey_to_db(library.db.clone(), &bundle.verification_key).await?;

				Ok(bundle.secret_key.expose().clone())
			})
		})
}
