import assert from 'node:assert/strict';
import test from 'node:test';
import type {
  AgentHealth,
  AgentHostConnectivityReason,
  AgentHostConnectivityStatus,
  AgentNetworkInterfaceKind,
  AgentPlatformReadinessReason,
  AgentProcessPrivilegeStatus,
  AgentSystemDnsVerificationStatus,
  AgentTunPermissionReadiness,
  AgentTunVerificationStatus,
} from '@chimera/interface';
import { overwriteGetLocale } from '@/paraglide/runtime';
import {
  presentHostConnectivityReason,
  presentHostConnectivityStatus,
  presentNetworkInterfaceKind,
  presentPlatformReadinessReason,
  presentProcessPrivilege,
  presentSystemDnsVerification,
  presentTunPermissionReadiness,
  presentTunVerification,
  presentYesNo,
} from './presenter';

overwriteGetLocale(() => 'en');

test('host connectivity statuses have stable labels and health levels', () => {
  const cases: Array<[AgentHostConnectivityStatus, string, AgentHealth]> = [
    ['online_dual_stack', 'Online over IPv4 and IPv6', 'healthy'],
    ['online_ipv4_only', 'Online over IPv4 only', 'healthy'],
    ['online_ipv6_only', 'Online over IPv6 only', 'healthy'],
    ['link_disconnected', 'Network link disconnected', 'critical'],
    ['address_unavailable', 'No usable IP address', 'critical'],
    ['default_route_unavailable', 'Default route unavailable', 'critical'],
    ['dns_unavailable', 'DNS unavailable', 'critical'],
    ['captive_portal_suspected', 'Captive portal suspected', 'warning'],
    ['internet_unreachable', 'Internet unreachable', 'critical'],
    ['indeterminate', 'Status could not be determined', 'degraded'],
  ];

  for (const [status, label, health] of cases) {
    assert.deepEqual(presentHostConnectivityStatus(status), { label, health });
  }
});

test('host connectivity reasons map to stable privacy-safe explanations', () => {
  const cases: Array<[AgentHostConnectivityReason, string]> = [
    ['probe_unavailable', 'The host connectivity probe is unavailable.'],
    ['no_active_interface', 'No active network interface was detected.'],
    ['wireless_disconnected', 'The wireless interface is disconnected.'],
    ['ethernet_disconnected', 'The Ethernet interface is disconnected.'],
    ['no_usable_ipv4_address', 'No usable IPv4 address was obtained.'],
    ['no_usable_ipv6_address', 'No usable IPv6 address was obtained.'],
    ['no_ipv4_default_route', 'No IPv4 default route is available.'],
    ['no_ipv6_default_route', 'No IPv6 default route is available.'],
    ['dns_not_configured', 'DNS is not configured on an active interface.'],
    ['dns_resolution_failed', 'DNS resolution failed.'],
    ['ipv4_internet_unreachable', 'The internet is unreachable over IPv4.'],
    ['ipv6_internet_unreachable', 'The internet is unreachable over IPv6.'],
    [
      'captive_portal_suspected',
      'A captive portal or HTTP interception is suspected.',
    ],
  ];

  for (const [reason, label] of cases) {
    assert.equal(presentHostConnectivityReason(reason), label);
  }
});

test('platform readiness statuses have stable labels and health levels', () => {
  const privileges: Array<[AgentProcessPrivilegeStatus, string, AgentHealth]> =
    [
      ['elevated', 'Elevated', 'healthy'],
      ['standard', 'Standard', 'warning'],
      ['unknown', 'Unknown', 'degraded'],
    ];
  const permissions: Array<[AgentTunPermissionReadiness, string, AgentHealth]> =
    [
      ['not_required', 'Not required', 'healthy'],
      ['satisfied', 'Satisfied', 'healthy'],
      [
        'service_alternative_available',
        'Satisfied by privileged service',
        'healthy',
      ],
      ['required', 'Administrator permission required', 'critical'],
      ['indeterminate', 'Could not be determined', 'degraded'],
    ];
  const tunStates: Array<[AgentTunVerificationStatus, string, AgentHealth]> = [
    ['not_requested', 'TUN not requested', 'healthy'],
    ['verified', 'Verified', 'healthy'],
    ['inconsistent', 'Inconsistent', 'critical'],
    ['unavailable', 'Unavailable', 'degraded'],
  ];
  const dnsStates: Array<
    [AgentSystemDnsVerificationStatus, string, AgentHealth]
  > = [
    ['not_required', 'Not required', 'healthy'],
    ['verified', 'Verified', 'healthy'],
    ['not_configured', 'Not configured', 'critical'],
    ['resolution_failed', 'Resolution failed', 'critical'],
    ['unavailable', 'Unavailable', 'degraded'],
  ];

  for (const [status, label, health] of privileges) {
    assert.deepEqual(presentProcessPrivilege(status), { label, health });
  }
  for (const [status, label, health] of permissions) {
    assert.deepEqual(presentTunPermissionReadiness(status), { label, health });
  }
  for (const [status, label, health] of tunStates) {
    assert.deepEqual(presentTunVerification(status), { label, health });
  }
  for (const [status, label, health] of dnsStates) {
    assert.deepEqual(presentSystemDnsVerification(status), { label, health });
  }
});

test('platform readiness reasons stay closed and privacy safe', () => {
  const cases: Array<[AgentPlatformReadinessReason, string]> = [
    [
      'privilege_probe_unavailable',
      'Process privilege could not be determined.',
    ],
    ['elevated_process', 'The current process has elevated permission.'],
    ['service_mode_active', 'The compatible privileged service is active.'],
    ['service_mode_available', 'A compatible privileged service is available.'],
    [
      'permission_required',
      'TUN requires administrator permission or a compatible privileged service.',
    ],
    ['tun_state_unavailable', 'The running core did not expose its TUN state.'],
    [
      'tun_state_inconsistent',
      'The requested and observed TUN states do not match.',
    ],
    [
      'system_dns_not_configured',
      'System DNS is not configured while TUN is requested.',
    ],
    [
      'system_dns_resolution_failed',
      'System DNS could not resolve names while TUN is requested.',
    ],
    [
      'system_dns_unavailable',
      'System DNS verification is unavailable while TUN is requested.',
    ],
  ];

  for (const [reason, label] of cases) {
    assert.equal(presentPlatformReadinessReason(reason), label);
  }
});

test('network interface and nullable boolean projections stay closed', () => {
  const interfaces: Array<[AgentNetworkInterfaceKind, string]> = [
    ['wireless', 'Wireless'],
    ['ethernet', 'Ethernet'],
    ['multiple', 'Multiple interfaces'],
    ['other', 'Other interface'],
    ['none', 'No active interface'],
    ['unknown', 'Unknown interface'],
  ];

  for (const [kind, label] of interfaces) {
    assert.equal(presentNetworkInterfaceKind(kind), label);
  }
  assert.equal(presentYesNo(true), 'Yes');
  assert.equal(presentYesNo(false), 'No');
  assert.equal(presentYesNo(null), 'Unknown');
});
