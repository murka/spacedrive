import { useLibraryQuery } from '@sd/client';
import { Button, Dialog, Input, Select, SelectOption } from '@sd/ui';
import { writeText } from '@tauri-apps/api/clipboard';
import { Buffer } from 'buffer';
import { Clipboard } from 'phosphor-react';
import { ReactNode, useState } from 'react';

import { getHashingAlgorithmString } from '../../screens/settings/library/KeysSetting';
import { SelectOptionKeyList } from '../key/KeyList';

interface KeyViewerDialogProps {
	trigger: ReactNode;
}

export const KeyUpdater = (props: {
	uuid: string;
	setKey: (value: string) => void;
	setEncryptionAlgo: (value: string) => void;
	setHashingAlgo: (value: string) => void;
	setContentSalt: (value: string) => void;
}) => {
	useLibraryQuery(['keys.getKey', props.uuid], {
		onSuccess: (data) => {
			props.setKey(data);
		}
	});

	const keys = useLibraryQuery(['keys.list']);

	const key = keys.data?.find((key) => key.uuid == props.uuid);
	key && props.setEncryptionAlgo(key?.algorithm);
	key && props.setHashingAlgo(getHashingAlgorithmString(key?.hashing_algorithm));
	key && props.setContentSalt(Buffer.from(key.content_salt).toString('hex'));

	return <></>;
};

export const KeyViewerDialog = (props: KeyViewerDialogProps) => {
	const keys = useLibraryQuery(['keys.list'], {
		onSuccess: (data) => {
			if (key === '' && data.length !== 0) {
				setKey(data[0].uuid);
			}
		}
	});

	const [showKeyViewerDialog, setShowKeyViewerDialog] = useState(false);
	const [key, setKey] = useState('');
	const [keyValue, setKeyValue] = useState('');
	const [contentSalt, setContentSalt] = useState('');
	const [encryptionAlgo, setEncryptionAlgo] = useState('');
	const [hashingAlgo, setHashingAlgo] = useState('');

	return (
		<>
			<Dialog
				open={showKeyViewerDialog}
				setOpen={setShowKeyViewerDialog}
				trigger={props.trigger}
				title="View Key Values"
				description="Here you can view the values of your keys."
				ctaLabel="Done"
				ctaAction={() => {
					setShowKeyViewerDialog(false);
				}}
			>
				<KeyUpdater
					uuid={key}
					setKey={setKeyValue}
					setEncryptionAlgo={setEncryptionAlgo}
					setHashingAlgo={setHashingAlgo}
					setContentSalt={setContentSalt}
				/>

				<div className="grid w-full gap-4 mt-4 mb-3">
					<div className="flex flex-col">
						<span className="text-xs font-bold">Key</span>
						<Select
							className="mt-2 flex-grow"
							value={key}
							onChange={(e) => {
								setKey(e);
							}}
						>
							{keys.data && <SelectOptionKeyList keys={keys.data.map((key) => key.uuid)} />}
						</Select>
					</div>
				</div>
				<div className="grid w-full grid-cols-2 gap-4 mt-4 mb-3">
					<div className="flex flex-col">
						<span className="text-xs font-bold">Encryption</span>
						<Select
							className="mt-2 text-gray-300"
							value={encryptionAlgo}
							disabled
							onChange={() => {}}
						>
							<SelectOption value="XChaCha20Poly1305">XChaCha20-Poly1305</SelectOption>
							<SelectOption value="Aes256Gcm">AES-256-GCM</SelectOption>
						</Select>
					</div>
					<div className="flex flex-col">
						<span className="text-xs font-bold">Hashing</span>
						<Select className="mt-2 text-gray-300" value={hashingAlgo} disabled onChange={() => {}}>
							<SelectOption value="Argon2id-s">Argon2id (standard)</SelectOption>
							<SelectOption value="Argon2id-h">Argon2id (hardened)</SelectOption>
							<SelectOption value="Argon2id-p">Argon2id (paranoid)</SelectOption>
						</Select>
					</div>
				</div>
				<div className="grid w-full gap-4 mt-4 mb-3">
					<div className="flex flex-col">
						<span className="text-xs font-bold mb-2">Content Salt (hex)</span>
						<div className="relative flex flex-grow">
							<Input value={contentSalt} disabled className="flex-grow !py-0.5" />
							<Button
								type="button"
								onClick={() => {
									writeText(contentSalt);
								}}
								size="icon"
								className="border-none absolute right-[5px] top-[5px]"
							>
								<Clipboard className="w-4 h-4" />
							</Button>
						</div>
					</div>
				</div>
				<div className="grid w-full gap-4 mt-4 mb-3">
					<div className="flex flex-col">
						<span className="text-xs font-bold mb-2">Key Value</span>
						<div className="relative flex flex-grow">
							<Input value={keyValue} disabled className="flex-grow !py-0.5" />
							<Button
								type="button"
								onClick={() => {
									writeText(keyValue);
								}}
								size="icon"
								className="border-none absolute right-[5px] top-[5px]"
							>
								<Clipboard className="w-4 h-4" />
							</Button>
						</div>
					</div>
				</div>
			</Dialog>
		</>
	);
};
