import { writable, derived } from 'svelte/store';
import type { BacnetPoint } from './api';

export const points = writable<BacnetPoint[]>([]);
export const deviceId = writable<number>(0);
export const filterText = writable('');
export const activeTab = writable<string>('all');

export type TabDef = { key: string; label: string; filter: (p: BacnetPoint) => boolean };

export const tabs: TabDef[] = [
	{ key: 'all', label: 'All', filter: () => true },
	{ key: 'analog', label: 'Analog', filter: (p) => p.objectType.startsWith('analog') },
	{ key: 'binary', label: 'Binary', filter: (p) => p.objectType.startsWith('binary') },
	{ key: 'multistate', label: 'Multi-State', filter: (p) => p.objectType.startsWith('multi-state') },
	{ key: 'input', label: 'Input', filter: (p) => p.objectType.endsWith('-input') },
	{ key: 'output', label: 'Output', filter: (p) => p.objectType.endsWith('-output') },
	{ key: 'value', label: 'Value', filter: (p) => p.objectType.endsWith('-value') },
];

export const filteredPoints = derived(
	[points, filterText, activeTab],
	([$points, $filterText, $activeTab]) => {
		const tab = tabs.find(t => t.key === $activeTab) ?? tabs[0];
		let result = $points.filter(tab.filter);
		if ($filterText) {
			try {
				const re = new RegExp($filterText, 'i');
				result = result.filter(p =>
					re.test(p.objectName) || re.test(p.description) || re.test(p.objectType)
				);
			} catch {
				const lower = $filterText.toLowerCase();
				result = result.filter(p =>
					p.objectName.toLowerCase().includes(lower) ||
					p.description.toLowerCase().includes(lower)
				);
			}
		}
		return result;
	}
);
