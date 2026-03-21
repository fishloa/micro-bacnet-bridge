<script lang="ts">
	import '../app.css';
	import { page } from '$app/state';
	import LayoutDashboard from 'lucide-svelte/icons/layout-dashboard';
	import Settings from 'lucide-svelte/icons/settings';
	import CircuitBoard from 'lucide-svelte/icons/circuit-board';
	import ArrowRightLeft from 'lucide-svelte/icons/arrow-right-left';
	import Users from 'lucide-svelte/icons/users';
	import KeyRound from 'lucide-svelte/icons/key-round';
	import Cpu from 'lucide-svelte/icons/cpu';
	import Info from 'lucide-svelte/icons/info';

	let { children } = $props();

	const navItems = [
		{ href: '/', icon: LayoutDashboard, label: 'Dashboard' },
		{ href: '/config', icon: Settings, label: 'Config' },
		{ href: '/points', icon: CircuitBoard, label: 'Points' },
		{ href: '/convertors', icon: ArrowRightLeft, label: 'Convertors' },
		{ href: '/users', icon: Users, label: 'Users' },
		{ href: '/tokens', icon: KeyRound, label: 'Tokens' },
		{ href: '/firmware', icon: Cpu, label: 'Firmware' },
	];
</script>

<div class="app-shell">
	<nav class="app-nav">
		<a href="/" class="app-brand" title="BACnet Bridge">
			<img src="https://icomb.place/design-system/czernin.svg" alt="Icomb Place" width="28" height="28" />
		</a>
		{#each navItems as item}
			<a
				href={item.href}
				class:active={page.url.pathname === item.href}
				title={item.label}
			>
				<item.icon size={22} strokeWidth={1.5} />
			</a>
		{/each}
		<div class="nav-spacer"></div>
		<a href="/status" title="System Status" class:active={page.url.pathname === '/status'}>
			<Info size={22} strokeWidth={1.5} />
		</a>
	</nav>
	<main class="app-content">
		{@render children()}
	</main>
</div>
