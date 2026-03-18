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
	/** BACnet engineering unit code reported by the device. -1 if not discovered. */
	discoveredUnit: number;
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
	bacnetIpEnabled: boolean;
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

export interface NtpConfig {
	enabled: boolean;
	use_dhcp_servers: boolean;
	servers: string[];
	sync_interval_secs: number;
}

export interface SyslogConfig {
	enabled: boolean;
	server: string;
	port: number;
}

export interface MqttConfig {
	enabled: boolean;
	broker: string;
	port: number;
	client_id: string;
	username: string;
	password: string;
	topic_prefix: string;
	ha_discovery_enabled: boolean;
	ha_discovery_prefix: string;
	publish_points: string[];
}

export interface SnmpConfig {
	enabled: boolean;
	community: string;
}

export interface PointConfig {
	objectType: string;
	objectInstance: number;
	// Display/conversion
	scale: number;         // multiplier (default 1.0)
	offset: number;        // added after scale (default 0.0)
	engineeringUnit: number; // BACnet unit code (0-65535), 95 = no-units
	// Bridge routing
	bridgeToBacnetIp: boolean; // forward to BACnet/IP (default true)
	bridgeToMqtt: boolean;     // publish to MQTT (default true)
	// Visibility / exposure
	showOnDashboard: boolean;  // show this point on the dashboard (default true)
	exposeInApi: boolean;      // expose via REST API (default true)
	// State text labels for multi-state objects (1-based, index 0 = state 1)
	stateText: string[];
}

export const ENGINEERING_UNITS: { code: number; label: string }[] = [
	{ code: 95, label: 'No Units' },
	{ code: 62, label: '°C' },
	{ code: 64, label: '°F' },
	{ code: 63, label: 'K' },
	{ code: 98, label: '%' },
	{ code: 53, label: 'Pa' },
	{ code: 54, label: 'kPa' },
	{ code: 55, label: 'bar' },
	{ code: 56, label: 'psi' },
	{ code: 57, label: 'inH₂O' },
	{ code: 47, label: 'W' },
	{ code: 48, label: 'kW' },
	{ code: 49, label: 'MW' },
	{ code: 19, label: 'kWh' },
	{ code: 50, label: 'BTU/h' },
	{ code: 3, label: 'A' },
	{ code: 2, label: 'mA' },
	{ code: 5, label: 'V' },
	{ code: 27, label: 'Hz' },
	{ code: 84, label: 'CFM' },
	{ code: 89, label: 'L/s' },
	{ code: 88, label: 'L/min' },
	{ code: 31, label: 'm' },
	{ code: 33, label: 'ft' },
	{ code: 74, label: 'm/s' },
	{ code: 75, label: 'L' },
	{ code: 39, label: 'kg' },
	{ code: 40, label: 'lb' },
	{ code: 73, label: 's' },
	{ code: 72, label: 'min' },
	{ code: 71, label: 'h' },
	{ code: 104, label: 'RPM' },
	{ code: 90, label: '°' },
	{ code: 96, label: 'ppm' },
];

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
		{ objectType: 'analog-input', objectInstance: 0, objectName: 'Supply Air Temp', description: 'AHU-1 supply air temperature sensor', presentValue: 55.2, units: '°C', writable: false, statusFlags: [], discoveredUnit: 62 },
		{ objectType: 'analog-input', objectInstance: 1, objectName: 'Return Air Temp', description: 'AHU-1 return air temperature sensor', presentValue: 72.8, units: '°C', writable: false, statusFlags: [], discoveredUnit: 62 },
		{ objectType: 'analog-input', objectInstance: 2, objectName: 'Mixed Air Temp', description: 'Mixed air temperature', presentValue: 63.1, units: '°C', writable: false, statusFlags: [], discoveredUnit: 62 },
		{ objectType: 'analog-input', objectInstance: 3, objectName: 'Outside Air Temp', description: 'Outside air temperature from OAT sensor', presentValue: 42.5, units: '°C', writable: false, statusFlags: [], discoveredUnit: 62 },
		{ objectType: 'analog-input', objectInstance: 4, objectName: 'Supply Air Pressure', description: 'Duct static pressure', presentValue: 1.25, units: 'inH₂O', writable: false, statusFlags: [], discoveredUnit: 57 },
		{ objectType: 'analog-output', objectInstance: 0, objectName: 'Cooling Valve', description: 'CHW valve command', presentValue: 45.0, units: '%', writable: true, statusFlags: [], discoveredUnit: 98 },
		{ objectType: 'analog-output', objectInstance: 1, objectName: 'Heating Valve', description: 'HHW valve command', presentValue: 0.0, units: '%', writable: true, statusFlags: [], discoveredUnit: 98 },
		{ objectType: 'analog-output', objectInstance: 2, objectName: 'Supply Fan Speed', description: 'Supply fan VFD command', presentValue: 72.0, units: '%', writable: true, statusFlags: [], discoveredUnit: 98 },
		{ objectType: 'analog-output', objectInstance: 3, objectName: 'Return Fan Speed', description: 'Return fan VFD command', presentValue: 68.0, units: '%', writable: true, statusFlags: [], discoveredUnit: 98 },
		{ objectType: 'analog-output', objectInstance: 4, objectName: 'OA Damper', description: 'Outside air damper position', presentValue: 35.0, units: '%', writable: true, statusFlags: [], discoveredUnit: 98 },
		{ objectType: 'analog-value', objectInstance: 0, objectName: 'SAT Setpoint', description: 'Supply air temp setpoint', presentValue: 55.0, units: '°C', writable: true, statusFlags: [], discoveredUnit: 62 },
		{ objectType: 'analog-value', objectInstance: 1, objectName: 'Static Pressure SP', description: 'Duct static pressure setpoint', presentValue: 1.5, units: 'inH₂O', writable: true, statusFlags: [], discoveredUnit: 57 },
		{ objectType: 'binary-input', objectInstance: 0, objectName: 'Supply Fan Status', description: 'Supply fan running status', presentValue: true, units: '', writable: false, statusFlags: [], discoveredUnit: 95 },
		{ objectType: 'binary-input', objectInstance: 1, objectName: 'Return Fan Status', description: 'Return fan running status', presentValue: true, units: '', writable: false, statusFlags: [], discoveredUnit: 95 },
		{ objectType: 'binary-input', objectInstance: 2, objectName: 'Filter DP Alarm', description: 'Filter differential pressure alarm', presentValue: false, units: '', writable: false, statusFlags: [], discoveredUnit: 95 },
		{ objectType: 'binary-input', objectInstance: 3, objectName: 'Freeze Stat', description: 'Freeze protection thermostat', presentValue: false, units: '', writable: false, statusFlags: [], discoveredUnit: 95 },
		{ objectType: 'binary-output', objectInstance: 0, objectName: 'Supply Fan Command', description: 'Supply fan start/stop', presentValue: true, units: '', writable: true, statusFlags: [], discoveredUnit: 95 },
		{ objectType: 'binary-output', objectInstance: 1, objectName: 'Return Fan Command', description: 'Return fan start/stop', presentValue: true, units: '', writable: true, statusFlags: [], discoveredUnit: 95 },
		{ objectType: 'binary-value', objectInstance: 0, objectName: 'Occupied Mode', description: 'Occupied/unoccupied schedule override', presentValue: true, units: '', writable: true, statusFlags: [], discoveredUnit: 95 },
		{ objectType: 'multi-state-input', objectInstance: 0, objectName: 'System Mode', description: '1=Off, 2=Heat, 3=Cool, 4=Auto', presentValue: 4, units: '', writable: false, statusFlags: [], discoveredUnit: 95 },
		{ objectType: 'multi-state-value', objectInstance: 0, objectName: 'Operating Mode', description: '1=Manual, 2=Auto, 3=Override', presentValue: 2, units: '', writable: true, statusFlags: [], discoveredUnit: 95 },
	],
	101: [
		{ objectType: 'analog-input', objectInstance: 0, objectName: 'Zone Temp', description: 'VAV zone temperature', presentValue: 73.4, units: '°C', writable: false, statusFlags: [], discoveredUnit: 62 },
		{ objectType: 'analog-input', objectInstance: 1, objectName: 'Airflow', description: 'Measured airflow', presentValue: 320, units: 'CFM', writable: false, statusFlags: [], discoveredUnit: 95 },
		{ objectType: 'analog-output', objectInstance: 0, objectName: 'Damper Position', description: 'VAV damper actuator', presentValue: 62.0, units: '%', writable: true, statusFlags: [], discoveredUnit: 98 },
		{ objectType: 'analog-output', objectInstance: 1, objectName: 'Reheat Valve', description: 'Reheat HHW valve', presentValue: 0.0, units: '%', writable: true, statusFlags: [], discoveredUnit: 98 },
		{ objectType: 'analog-value', objectInstance: 0, objectName: 'Zone Temp SP', description: 'Zone temperature setpoint', presentValue: 72.0, units: '°C', writable: true, statusFlags: [], discoveredUnit: 62 },
		{ objectType: 'analog-value', objectInstance: 1, objectName: 'Airflow Min', description: 'Minimum airflow setpoint', presentValue: 150, units: 'CFM', writable: true, statusFlags: [], discoveredUnit: 95 },
		{ objectType: 'analog-value', objectInstance: 2, objectName: 'Airflow Max', description: 'Maximum airflow setpoint', presentValue: 800, units: 'CFM', writable: true, statusFlags: [], discoveredUnit: 95 },
		{ objectType: 'binary-input', objectInstance: 0, objectName: 'Occupancy', description: 'Occupancy sensor', presentValue: true, units: '', writable: false, statusFlags: [], discoveredUnit: 95 },
		{ objectType: 'binary-value', objectInstance: 0, objectName: 'Override', description: 'Manual override active', presentValue: false, units: '', writable: true, statusFlags: [], discoveredUnit: 95 },
	],
	102: [
		{ objectType: 'analog-input', objectInstance: 0, objectName: 'Zone Temp', description: 'VAV zone temperature', presentValue: 71.8, units: '°C', writable: false, statusFlags: [], discoveredUnit: 62 },
		{ objectType: 'analog-input', objectInstance: 1, objectName: 'Airflow', description: 'Measured airflow', presentValue: 280, units: 'CFM', writable: false, statusFlags: [], discoveredUnit: 95 },
		{ objectType: 'analog-output', objectInstance: 0, objectName: 'Damper Position', description: 'VAV damper actuator', presentValue: 48.0, units: '%', writable: true, statusFlags: [], discoveredUnit: 98 },
		{ objectType: 'analog-value', objectInstance: 0, objectName: 'Zone Temp SP', description: 'Zone temperature setpoint', presentValue: 72.0, units: '°C', writable: true, statusFlags: [], discoveredUnit: 62 },
		{ objectType: 'binary-input', objectInstance: 0, objectName: 'Occupancy', description: 'Occupancy sensor', presentValue: false, units: '', writable: false, statusFlags: [], discoveredUnit: 95 },
	],
	200: [
		{ objectType: 'analog-input', objectInstance: 0, objectName: 'CHWS Temp', description: 'Chilled water supply temperature', presentValue: 44.2, units: '°C', writable: false, statusFlags: [], discoveredUnit: 62 },
		{ objectType: 'analog-input', objectInstance: 1, objectName: 'CHWR Temp', description: 'Chilled water return temperature', presentValue: 56.1, units: '°C', writable: false, statusFlags: [], discoveredUnit: 62 },
		{ objectType: 'analog-input', objectInstance: 2, objectName: 'CW Supply Temp', description: 'Condenser water supply', presentValue: 82.3, units: '°C', writable: false, statusFlags: [], discoveredUnit: 62 },
		{ objectType: 'analog-input', objectInstance: 3, objectName: 'Plant kW', description: 'Total chiller plant power', presentValue: 142.7, units: 'kW', writable: false, statusFlags: [], discoveredUnit: 95 },
		{ objectType: 'binary-input', objectInstance: 0, objectName: 'Chiller 1 Status', description: 'CH-1 running status', presentValue: true, units: '', writable: false, statusFlags: [], discoveredUnit: 95 },
		{ objectType: 'binary-input', objectInstance: 1, objectName: 'Chiller 2 Status', description: 'CH-2 running status', presentValue: false, units: '', writable: false, statusFlags: [], discoveredUnit: 95 },
		{ objectType: 'binary-output', objectInstance: 0, objectName: 'Chiller 1 Enable', description: 'CH-1 enable command', presentValue: true, units: '', writable: true, statusFlags: [], discoveredUnit: 95 },
		{ objectType: 'binary-output', objectInstance: 1, objectName: 'Chiller 2 Enable', description: 'CH-2 enable command', presentValue: false, units: '', writable: true, statusFlags: [], discoveredUnit: 95 },
		{ objectType: 'analog-value', objectInstance: 0, objectName: 'CHWS SP', description: 'Chilled water setpoint', presentValue: 44.0, units: '°C', writable: true, statusFlags: [], discoveredUnit: 62 },
	],
	201: [],
	300: [
		{ objectType: 'binary-output', objectInstance: 0, objectName: 'Zone 3A Lights', description: 'Lighting zone 3A on/off', presentValue: true, units: '', writable: true, statusFlags: [], discoveredUnit: 95 },
		{ objectType: 'binary-output', objectInstance: 1, objectName: 'Zone 3B Lights', description: 'Lighting zone 3B on/off', presentValue: true, units: '', writable: true, statusFlags: [], discoveredUnit: 95 },
		{ objectType: 'binary-output', objectInstance: 2, objectName: 'Zone 3C Lights', description: 'Lighting zone 3C on/off', presentValue: false, units: '', writable: true, statusFlags: [], discoveredUnit: 95 },
		{ objectType: 'analog-output', objectInstance: 0, objectName: 'Zone 3A Dimmer', description: 'Dimmer level zone 3A', presentValue: 85, units: '%', writable: true, statusFlags: [], discoveredUnit: 98 },
		{ objectType: 'analog-output', objectInstance: 1, objectName: 'Zone 3B Dimmer', description: 'Dimmer level zone 3B', presentValue: 100, units: '%', writable: true, statusFlags: [], discoveredUnit: 98 },
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
	bacnetIpEnabled: true,
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

const MOCK_NTP_CONFIG: NtpConfig = {
	enabled: true,
	use_dhcp_servers: true,
	servers: ['pool.ntp.org', '', ''],
	sync_interval_secs: 3600,
};

const MOCK_SYSLOG_CONFIG: SyslogConfig = {
	enabled: false,
	server: '',
	port: 514,
};

const MOCK_MQTT_CONFIG: MqttConfig = {
	enabled: false,
	broker: '',
	port: 1883,
	client_id: 'bacnet-bridge',
	username: '',
	password: '',
	topic_prefix: 'bacnet',
	ha_discovery_enabled: false,
	ha_discovery_prefix: 'homeassistant',
	publish_points: [],
};

const MOCK_SNMP_CONFIG: SnmpConfig = {
	enabled: false,
	community: 'public',
};

// Generate default PointConfig for every point in MOCK_POINTS[100]
const MOCK_POINT_CONFIGS: PointConfig[] = MOCK_POINTS[100].map(p => {
	const stateText: string[] =
		p.objectType === 'multi-state-input' && p.objectInstance === 0
			? ['Off', 'Heat', 'Cool', 'Auto']
			: p.objectType === 'multi-state-value' && p.objectInstance === 0
			? ['Manual', 'Auto', 'Override']
			: [];
	return {
		objectType: p.objectType,
		objectInstance: p.objectInstance,
		scale: 1.0,
		offset: 0.0,
		engineeringUnit: 95,
		bridgeToBacnetIp: true,
		bridgeToMqtt: true,
		showOnDashboard: true,
		exposeInApi: true,
		stateText,
	};
});

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
	getNtpConfig: () => get('/config/ntp', MOCK_NTP_CONFIG),
	setNtpConfig: (cfg: NtpConfig) => put('/config/ntp', cfg, cfg),
	getSyslogConfig: () => get('/config/syslog', MOCK_SYSLOG_CONFIG),
	setSyslogConfig: (cfg: SyslogConfig) => put('/config/syslog', cfg, cfg),
	getMqttConfig: () => get('/config/mqtt', MOCK_MQTT_CONFIG),
	setMqttConfig: (cfg: MqttConfig) => put('/config/mqtt', cfg, cfg),
	getSnmpConfig: () => get('/config/snmp', MOCK_SNMP_CONFIG),
	setSnmpConfig: (cfg: SnmpConfig) => put('/config/snmp', cfg, cfg),
	getPointConfigs: () => get('/config/points', MOCK_POINT_CONFIGS),
	setPointConfig: (objectType: string, objectInstance: number, cfg: PointConfig) =>
		put(`/config/points/${objectType}:${objectInstance}`, cfg, cfg),
	setPointConfigs: (configs: PointConfig[]) =>
		put('/config/points', configs, configs),
};

export function pointKey(p: BacnetPoint): string {
	return `${p.objectType}:${p.objectInstance}`;
}

export function isNumericType(objectType: string): boolean {
	return (
		objectType === 'analog-input' ||
		objectType === 'analog-output' ||
		objectType === 'analog-value' ||
		objectType === 'multi-state-input' ||
		objectType === 'multi-state-output' ||
		objectType === 'multi-state-value'
	);
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
