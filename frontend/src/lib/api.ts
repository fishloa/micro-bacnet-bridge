// API client for micro-bacnet-bridge REST API
// In dev mode, returns mock data. In production, hits /api/v1/*.

export interface BacnetDevice {
	id: number;
	name: string;
	network: number;
	mac: string;
	vendor: string;
	model: string;
	online: boolean;
}

export interface BacnetPoint {
	objectType: string;
	objectInstance: number;
	objectName: string;
	description: string;
	presentValue: string | number | boolean;
	units: string;
	writable: boolean;
	statusFlags: string[];
}

export interface NetworkConfig {
	dhcp: boolean;
	ip: string;
	subnet: string;
	gateway: string;
	dns: string;
	hostname: string;
}

export interface BacnetConfig {
	deviceId: number;
	deviceName: string;
	vendor: string;
	mstpMac: number;
	mstpBaud: number;
	maxMaster: number;
}

export interface SystemStatus {
	uptime: number;
	ip: string;
	dhcp: boolean;
	hostname: string;
	firmwareVersion: string;
	mstpState: string;
	mstpFramesSent: number;
	mstpFramesRecv: number;
	devicesDiscovered: number;
}

export interface User {
	id: number;
	username: string;
	role: 'admin' | 'viewer';
}

// Object type display info
export const OBJECT_TYPE_INFO: Record<string, { label: string; color: string }> = {
	'analog-input':    { label: 'AI',  color: 'info' },
	'analog-output':   { label: 'AO',  color: 'info' },
	'analog-value':    { label: 'AV',  color: 'info' },
	'binary-input':    { label: 'BI',  color: 'success' },
	'binary-output':   { label: 'BO',  color: 'success' },
	'binary-value':    { label: 'BV',  color: 'success' },
	'multi-state-input':  { label: 'MSI', color: 'purple' },
	'multi-state-output': { label: 'MSO', color: 'purple' },
	'multi-state-value':  { label: 'MSV', color: 'purple' },
	'notification-class': { label: 'NC',  color: 'warning' },
	'trend-log':       { label: 'TL',  color: 'warning' },
	'schedule':        { label: 'SCH', color: 'danger' },
	'calendar':        { label: 'CAL', color: 'danger' },
};

// --- Mock data for development ---

const MOCK_DEVICES: BacnetDevice[] = [
	{ id: 100, name: 'AHU-1 Controller', network: 1, mac: '0A', vendor: 'Johnson Controls', model: 'FX-PCG', online: true },
	{ id: 101, name: 'VAV-3.01', network: 1, mac: '0B', vendor: 'Tridium', model: 'JACE-8000', online: true },
	{ id: 102, name: 'VAV-3.02', network: 1, mac: '0C', vendor: 'Tridium', model: 'JACE-8000', online: true },
	{ id: 200, name: 'Chiller Plant', network: 1, mac: '14', vendor: 'Carrier', model: 'i-Vu CCN', online: true },
	{ id: 201, name: 'Boiler Room', network: 1, mac: '15', vendor: 'Honeywell', model: 'Spyder', online: false },
	{ id: 300, name: 'Lighting Panel L3', network: 1, mac: '1E', vendor: 'Lutron', model: 'QS-BACnet', online: true },
];

const MOCK_POINTS: Record<number, BacnetPoint[]> = {
	100: [
		{ objectType: 'analog-input', objectInstance: 0, objectName: 'Supply Air Temp', description: 'AHU-1 supply air temperature sensor', presentValue: 55.2, units: '°C', writable: false, statusFlags: [] },
		{ objectType: 'analog-input', objectInstance: 1, objectName: 'Return Air Temp', description: 'AHU-1 return air temperature sensor', presentValue: 72.8, units: '°C', writable: false, statusFlags: [] },
		{ objectType: 'analog-input', objectInstance: 2, objectName: 'Mixed Air Temp', description: 'Mixed air temperature', presentValue: 63.1, units: '°C', writable: false, statusFlags: [] },
		{ objectType: 'analog-input', objectInstance: 3, objectName: 'Outside Air Temp', description: 'Outside air temperature from OAT sensor', presentValue: 42.5, units: '°C', writable: false, statusFlags: [] },
		{ objectType: 'analog-input', objectInstance: 4, objectName: 'Supply Air Pressure', description: 'Duct static pressure', presentValue: 1.25, units: 'inH₂O', writable: false, statusFlags: [] },
		{ objectType: 'analog-output', objectInstance: 0, objectName: 'Cooling Valve', description: 'CHW valve command', presentValue: 45.0, units: '%', writable: true, statusFlags: [] },
		{ objectType: 'analog-output', objectInstance: 1, objectName: 'Heating Valve', description: 'HHW valve command', presentValue: 0.0, units: '%', writable: true, statusFlags: [] },
		{ objectType: 'analog-output', objectInstance: 2, objectName: 'Supply Fan Speed', description: 'Supply fan VFD command', presentValue: 72.0, units: '%', writable: true, statusFlags: [] },
		{ objectType: 'analog-output', objectInstance: 3, objectName: 'Return Fan Speed', description: 'Return fan VFD command', presentValue: 68.0, units: '%', writable: true, statusFlags: [] },
		{ objectType: 'analog-output', objectInstance: 4, objectName: 'OA Damper', description: 'Outside air damper position', presentValue: 35.0, units: '%', writable: true, statusFlags: [] },
		{ objectType: 'analog-value', objectInstance: 0, objectName: 'SAT Setpoint', description: 'Supply air temp setpoint', presentValue: 55.0, units: '°C', writable: true, statusFlags: [] },
		{ objectType: 'analog-value', objectInstance: 1, objectName: 'Static Pressure SP', description: 'Duct static pressure setpoint', presentValue: 1.5, units: 'inH₂O', writable: true, statusFlags: [] },
		{ objectType: 'binary-input', objectInstance: 0, objectName: 'Supply Fan Status', description: 'Supply fan running status', presentValue: true, units: '', writable: false, statusFlags: [] },
		{ objectType: 'binary-input', objectInstance: 1, objectName: 'Return Fan Status', description: 'Return fan running status', presentValue: true, units: '', writable: false, statusFlags: [] },
		{ objectType: 'binary-input', objectInstance: 2, objectName: 'Filter DP Alarm', description: 'Filter differential pressure alarm', presentValue: false, units: '', writable: false, statusFlags: [] },
		{ objectType: 'binary-input', objectInstance: 3, objectName: 'Freeze Stat', description: 'Freeze protection thermostat', presentValue: false, units: '', writable: false, statusFlags: [] },
		{ objectType: 'binary-output', objectInstance: 0, objectName: 'Supply Fan Command', description: 'Supply fan start/stop', presentValue: true, units: '', writable: true, statusFlags: [] },
		{ objectType: 'binary-output', objectInstance: 1, objectName: 'Return Fan Command', description: 'Return fan start/stop', presentValue: true, units: '', writable: true, statusFlags: [] },
		{ objectType: 'binary-value', objectInstance: 0, objectName: 'Occupied Mode', description: 'Occupied/unoccupied schedule override', presentValue: true, units: '', writable: true, statusFlags: [] },
		{ objectType: 'multi-state-input', objectInstance: 0, objectName: 'System Mode', description: '1=Off, 2=Heat, 3=Cool, 4=Auto', presentValue: 4, units: '', writable: false, statusFlags: [] },
		{ objectType: 'multi-state-value', objectInstance: 0, objectName: 'Operating Mode', description: '1=Manual, 2=Auto, 3=Override', presentValue: 2, units: '', writable: true, statusFlags: [] },
	],
	101: [
		{ objectType: 'analog-input', objectInstance: 0, objectName: 'Zone Temp', description: 'VAV zone temperature', presentValue: 73.4, units: '°C', writable: false, statusFlags: [] },
		{ objectType: 'analog-input', objectInstance: 1, objectName: 'Airflow', description: 'Measured airflow', presentValue: 320, units: 'CFM', writable: false, statusFlags: [] },
		{ objectType: 'analog-output', objectInstance: 0, objectName: 'Damper Position', description: 'VAV damper actuator', presentValue: 62.0, units: '%', writable: true, statusFlags: [] },
		{ objectType: 'analog-output', objectInstance: 1, objectName: 'Reheat Valve', description: 'Reheat HHW valve', presentValue: 0.0, units: '%', writable: true, statusFlags: [] },
		{ objectType: 'analog-value', objectInstance: 0, objectName: 'Zone Temp SP', description: 'Zone temperature setpoint', presentValue: 72.0, units: '°C', writable: true, statusFlags: [] },
		{ objectType: 'analog-value', objectInstance: 1, objectName: 'Airflow Min', description: 'Minimum airflow setpoint', presentValue: 150, units: 'CFM', writable: true, statusFlags: [] },
		{ objectType: 'analog-value', objectInstance: 2, objectName: 'Airflow Max', description: 'Maximum airflow setpoint', presentValue: 800, units: 'CFM', writable: true, statusFlags: [] },
		{ objectType: 'binary-input', objectInstance: 0, objectName: 'Occupancy', description: 'Occupancy sensor', presentValue: true, units: '', writable: false, statusFlags: [] },
		{ objectType: 'binary-value', objectInstance: 0, objectName: 'Override', description: 'Manual override active', presentValue: false, units: '', writable: true, statusFlags: [] },
	],
	102: [
		{ objectType: 'analog-input', objectInstance: 0, objectName: 'Zone Temp', description: 'VAV zone temperature', presentValue: 71.8, units: '°C', writable: false, statusFlags: [] },
		{ objectType: 'analog-input', objectInstance: 1, objectName: 'Airflow', description: 'Measured airflow', presentValue: 280, units: 'CFM', writable: false, statusFlags: [] },
		{ objectType: 'analog-output', objectInstance: 0, objectName: 'Damper Position', description: 'VAV damper actuator', presentValue: 48.0, units: '%', writable: true, statusFlags: [] },
		{ objectType: 'analog-value', objectInstance: 0, objectName: 'Zone Temp SP', description: 'Zone temperature setpoint', presentValue: 72.0, units: '°C', writable: true, statusFlags: [] },
		{ objectType: 'binary-input', objectInstance: 0, objectName: 'Occupancy', description: 'Occupancy sensor', presentValue: false, units: '', writable: false, statusFlags: [] },
	],
	200: [
		{ objectType: 'analog-input', objectInstance: 0, objectName: 'CHWS Temp', description: 'Chilled water supply temperature', presentValue: 44.2, units: '°C', writable: false, statusFlags: [] },
		{ objectType: 'analog-input', objectInstance: 1, objectName: 'CHWR Temp', description: 'Chilled water return temperature', presentValue: 56.1, units: '°C', writable: false, statusFlags: [] },
		{ objectType: 'analog-input', objectInstance: 2, objectName: 'CW Supply Temp', description: 'Condenser water supply', presentValue: 82.3, units: '°C', writable: false, statusFlags: [] },
		{ objectType: 'analog-input', objectInstance: 3, objectName: 'Plant kW', description: 'Total chiller plant power', presentValue: 142.7, units: 'kW', writable: false, statusFlags: [] },
		{ objectType: 'binary-input', objectInstance: 0, objectName: 'Chiller 1 Status', description: 'CH-1 running status', presentValue: true, units: '', writable: false, statusFlags: [] },
		{ objectType: 'binary-input', objectInstance: 1, objectName: 'Chiller 2 Status', description: 'CH-2 running status', presentValue: false, units: '', writable: false, statusFlags: [] },
		{ objectType: 'binary-output', objectInstance: 0, objectName: 'Chiller 1 Enable', description: 'CH-1 enable command', presentValue: true, units: '', writable: true, statusFlags: [] },
		{ objectType: 'binary-output', objectInstance: 1, objectName: 'Chiller 2 Enable', description: 'CH-2 enable command', presentValue: false, units: '', writable: true, statusFlags: [] },
		{ objectType: 'analog-value', objectInstance: 0, objectName: 'CHWS SP', description: 'Chilled water setpoint', presentValue: 44.0, units: '°C', writable: true, statusFlags: [] },
	],
	201: [],
	300: [
		{ objectType: 'binary-output', objectInstance: 0, objectName: 'Zone 3A Lights', description: 'Lighting zone 3A on/off', presentValue: true, units: '', writable: true, statusFlags: [] },
		{ objectType: 'binary-output', objectInstance: 1, objectName: 'Zone 3B Lights', description: 'Lighting zone 3B on/off', presentValue: true, units: '', writable: true, statusFlags: [] },
		{ objectType: 'binary-output', objectInstance: 2, objectName: 'Zone 3C Lights', description: 'Lighting zone 3C on/off', presentValue: false, units: '', writable: true, statusFlags: [] },
		{ objectType: 'analog-output', objectInstance: 0, objectName: 'Zone 3A Dimmer', description: 'Dimmer level zone 3A', presentValue: 85, units: '%', writable: true, statusFlags: [] },
		{ objectType: 'analog-output', objectInstance: 1, objectName: 'Zone 3B Dimmer', description: 'Dimmer level zone 3B', presentValue: 100, units: '%', writable: true, statusFlags: [] },
	],
};

const MOCK_NETWORK_CONFIG: NetworkConfig = {
	dhcp: true,
	ip: '192.168.1.42',
	subnet: '255.255.255.0',
	gateway: '192.168.1.1',
	dns: '192.168.1.1',
	hostname: 'bacnet-bridge',
};

const MOCK_BACNET_CONFIG: BacnetConfig = {
	deviceId: 389999,
	deviceName: 'BACnet Bridge',
	vendor: 'Icomb Place',
	mstpMac: 1,
	mstpBaud: 76800,
	maxMaster: 127,
};

const MOCK_STATUS: SystemStatus = {
	uptime: 86423,
	ip: '192.168.1.42',
	dhcp: true,
	hostname: 'bacnet-bridge',
	firmwareVersion: '0.1.0',
	mstpState: 'idle',
	mstpFramesSent: 142857,
	mstpFramesRecv: 298341,
	devicesDiscovered: 5,
};

const MOCK_USERS: User[] = [
	{ id: 1, username: 'admin', role: 'admin' },
	{ id: 2, username: 'operator', role: 'viewer' },
];

// --- API functions ---

import { dev } from '$app/environment';
const IS_DEV = dev;

async function get<T>(path: string, mock: T): Promise<T> {
	if (IS_DEV) return mock;
	const controller = new AbortController();
	const timer = setTimeout(() => controller.abort(), 8000);
	try {
		const res = await fetch(`/api/v1${path}`, { signal: controller.signal });
		if (!res.ok) throw new Error(`API error: ${res.status}`);
		return res.json();
	} finally {
		clearTimeout(timer);
	}
}

async function put<T>(path: string, body: unknown, mock: T): Promise<T> {
	if (IS_DEV) return mock;
	const controller = new AbortController();
	const timer = setTimeout(() => controller.abort(), 8000);
	try {
		const res = await fetch(`/api/v1${path}`, {
			method: 'PUT',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify(body),
			signal: controller.signal,
		});
		if (!res.ok) throw new Error(`API error: ${res.status}`);
		return res.json();
	} finally {
		clearTimeout(timer);
	}
}

export const api = {
	getDevices: () => get('/devices', MOCK_DEVICES),
	getPoints: (deviceId: number) => get(`/devices/${deviceId}/points`, MOCK_POINTS[deviceId] ?? []),
	getNetworkConfig: () => get('/config/network', MOCK_NETWORK_CONFIG),
	setNetworkConfig: (cfg: NetworkConfig) => put('/config/network', cfg, cfg),
	getBacnetConfig: () => get('/config/bacnet', MOCK_BACNET_CONFIG),
	setBacnetConfig: (cfg: BacnetConfig) => put('/config/bacnet', cfg, cfg),
	getStatus: () => get('/system/status', MOCK_STATUS),
	getUsers: () => get('/users', MOCK_USERS),
	writePoint: (deviceId: number, objectType: string, objectInstance: number, value: string | number | boolean) =>
		put(`/devices/${deviceId}/points/${objectType}:${objectInstance}`, { value }, { ok: true }),
};

export function pointKey(p: BacnetPoint): string {
	return `${p.objectType}:${p.objectInstance}`;
}

// SSE client for live point value updates
export function connectSSE(onUpdate: (updates: Record<string, number>) => void): () => void {
	const url = '/api/events';
	let es: EventSource | null = null;
	let retryTimer: ReturnType<typeof setTimeout> | null = null;

	function connect() {
		es = new EventSource(url);
		es.onmessage = (event) => {
			try {
				onUpdate(JSON.parse(event.data));
			} catch { /* ignore malformed */ }
		};
		es.onerror = () => {
			es?.close();
			retryTimer = setTimeout(connect, 3000);
		};
	}

	connect();
	return () => {
		es?.close();
		es = null;
		if (retryTimer) clearTimeout(retryTimer);
	};
}
