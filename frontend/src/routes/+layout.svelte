<script lang="ts">
	import '../app.css';
	import { page } from '$app/state';

	let { children } = $props();

	const navItems = [
		{ href: '/', icon: '⊞', label: 'Dashboard' },
		{ href: '/config', icon: '⚙', label: 'Config' },
		{ href: '/points', icon: '⊡', label: 'Points' },
		{ href: '/convertors', icon: '⊛', label: 'Convertors' },
		{ href: '/users', icon: '⊕', label: 'Users' },
		{ href: '/tokens', icon: '⊗', label: 'Tokens' },
		{ href: '/tls', icon: '⊘', label: 'TLS' },
		{ href: '/firmware', icon: '⊙', label: 'Firmware' },
	];
</script>

<div class="app-shell">
	<nav class="app-nav">
		<div class="app-brand">B</div>
		{#each navItems as item}
			<a
				href={item.href}
				class:active={page.url.pathname === item.href}
				title={item.label}
			>{item.icon}</a>
		{/each}
		<div class="nav-spacer"></div>
		<a href="/status" title="System Status" class:active={page.url.pathname === '/status'}>ℹ</a>
	</nav>
	<main class="app-content">
		{@render children()}
	</main>
</div>
