'use client';

import React, { useState, useEffect } from 'react';
import Link from 'next/link';
import { usePathname } from 'next/navigation';
import styled, { keyframes } from 'styled-components';
import { motion } from 'framer-motion';
import { useSwarmStore } from '../../lib/store/useSwarmStore';
import { fetchClusterDiagnostics } from '../../actions/admin';
import { History, LayoutDashboard, Network, Shield, Vault } from 'lucide-react';

const blinkHeartbeat = keyframes`
  0%, 100% { opacity: 1; transform: scale(1); }
  50% { opacity: 0.3; transform: scale(0.85); }
`;

const SidebarContainer = styled.aside`
  width: 256px;
  height: 100%;
  border-right: 1px solid #1f1f23;
  background-color: #09090b;
  display: flex;
  flex-direction: column;
  box-sizing: border-box;
  flex-shrink: 0;
  z-index: 40;
`;

const LogoSection = styled.div`
  padding: 24px 20px;
  display: flex;
  align-items: center;
  gap: 12px;
  border-bottom: 1px solid #1f1f23;
  background-color: #020202;
  box-sizing: border-box;
`;

const LogoWrapper = styled.div`
  width: 32px;
  height: 32px;
  display: flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
`;

const BrandName = styled.div`
  display: flex;
  flex-direction: column;
`;

const BrandTitle = styled.span`
  font-size: 15px;
  font-weight: 900;
  letter-spacing: 0.2em;
  color: #ffffff;
  font-family: monospace;
`;

const BrandSubtitle = styled.span`
  font-size: 9px;
  color: #00f3ff;
  letter-spacing: 0.15em;
  text-transform: uppercase;
  font-family: monospace;
`;

const ProfileSection = styled.div`
  padding: 16px 20px;
  border-bottom: 1px solid #1f1f23;
  background-color: #0c0c0e;
  display: flex;
  flex-direction: column;
  gap: 10px;
  box-sizing: border-box;
`;

const ProfileHeader = styled.div`
  font-size: 9px;
  color: #71717a;
  text-transform: uppercase;
  letter-spacing: 0.18em;
  font-weight: bold;
`;

const OperatorDetails = styled.div`
  display: flex;
  align-items: center;
  gap: 12px;
`;

const TerminalAvatar = styled.div<{ $isActive: boolean }>`
  width: 32px;
  height: 32px;
  border: 1px solid ${props => (props.$isActive ? '#00f3ff' : '#ff003c')};
  background-color: #09090b;
  display: flex;
  align-items: center;
  justify-content: center;
  position: relative;
  box-shadow: ${props =>
    props.$isActive
      ? '0 0 10px rgba(0, 243, 255, 0.2)'
      : '0 0 8px rgba(255, 0, 60, 0.15)'};

  &::after {
    content: '';
    position: absolute;
    inset: -3px;
    border: 1px dashed
      ${props =>
        props.$isActive ? 'rgba(0, 243, 255, 0.3)' : 'rgba(255, 0, 60, 0.2)'};
  }
`;

const AvatarText = styled.span<{ $isActive: boolean }>`
  font-size: 11px;
  color: ${props => (props.$isActive ? '#00f3ff' : '#ff003c')};
  font-weight: bold;
`;

const OperatorMeta = styled.div`
  display: flex;
  flex-direction: column;
  gap: 2px;
  overflow: hidden;
`;

const OperatorId = styled.span`
  font-size: 11px;
  font-weight: bold;
  color: #ffffff;
  text-overflow: ellipsis;
  overflow: hidden;
  white-space: nowrap;
`;

const OperatorStatus = styled.div`
  display: flex;
  align-items: center;
  gap: 6px;
`;

const HeartbeatDot = styled.span<{ $isActive: boolean }>`
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background-color: ${props => (props.$isActive ? '#10b981' : '#ff003c')};
  box-shadow: 0 0 6px ${props => (props.$isActive ? '#10b981' : '#ff003c')};
  animation: ${blinkHeartbeat} 1.5s infinite;
`;

const StatusText = styled.span<{ $isActive: boolean }>`
  font-size: 9px;
  color: ${props => (props.$isActive ? '#a1a1aa' : '#ff003c')};
  letter-spacing: 0.1em;
  text-transform: uppercase;
  font-weight: bold;
`;

const NavList = styled.nav`
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 16px;
  flex: 1;
  overflow-y: auto;
  box-sizing: border-box;

  &::-webkit-scrollbar {
    width: 4px;
  }
  &::-webkit-scrollbar-track {
    background: transparent;
  }
  &::-webkit-scrollbar-thumb {
    background: #1f1f23;
  }
`;

const NavLink = styled(Link)<{ $isActive: boolean }>`
  position: relative;
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 14px;
  color: ${props => (props.$isActive ? '#ffffff' : '#71717a')};
  text-decoration: none;
  font-size: 11px;
  font-weight: ${props => (props.$isActive ? 'bold' : 'normal')};
  letter-spacing: 0.12em;
  text-transform: uppercase;
  transition: color 0.2s;
  box-sizing: border-box;

  &:hover {
    color: #ffffff;
  }
`;

const ActiveLine = styled(motion.div)`
  position: absolute;
  left: 0;
  top: 12%;
  height: 76%;
  width: 2px;
  background-color: #00f3ff;
  box-shadow: 0 0 8px #00f3ff;
`;

const BottomSection = styled.div`
  padding: 16px 20px;
  border-top: 1px solid #1f1f23;
  background-color: #020202;
  display: flex;
  flex-direction: column;
  gap: 12px;
  box-sizing: border-box;
`;

const DiagnosticGroup = styled.div`
  display: flex;
  flex-direction: column;
  gap: 6px;
`;

const DiagnosticRow = styled.div`
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-size: 9px;
  font-family: monospace;
`;

const DiagnosticLabel = styled.span`
  color: #52525b;
  text-transform: uppercase;
  letter-spacing: 0.1em;
`;

const DiagnosticValue = styled.span<{ $alert?: boolean; $accent?: boolean }>`
  color: ${props =>
    props.$alert ? '#ef4444' : props.$accent ? '#00f3ff' : '#a1a1aa'};
  font-weight: bold;
`;

export function Sidebar() {
  const pathname = usePathname();
  const daemonOnline = useSwarmStore(state => state.daemonOnline);
  const currentTps = useSwarmStore(state => state.currentTps);
  const quarantinedAgents = useSwarmStore(state => state.quarantinedAgents);
  const activeTopology = useSwarmStore(state => state.activeTopology);

  const [clusterInfo, setClusterInfo] = useState<{
    node_id: string;
    wal_bytes: number;
    buffer_load: number;
  } | null>(null);

  useEffect(() => {
    fetchClusterDiagnostics().then(data => {
      if (data) {
        setClusterInfo(data);
      }
    });
  }, [daemonOnline]);

  const navLinks = [
    { href: '/', label: 'Dashboard', icon: LayoutDashboard },
    { href: '/topology', label: 'Topology', icon: Network },
    { href: '/aegis', label: 'Aegis Governance', icon: Shield },
    { href: '/vault', label: 'Audit Vault', icon: Vault },
    { href: '/replay', label: 'Time Travel // Replay', icon: History },
  ];

  return (
    <SidebarContainer>
      <LogoSection>
        <LogoWrapper>
          <svg
            width="44"
            height="44"
            viewBox="0 0 100 100"
            fill="none"
            xmlns="http://www.w3.org/2000/svg"
          >
            <rect
              x="5"
              y="5"
              width="90"
              height="90"
              stroke="#00f3ff"
              strokeWidth="2"
              fill="#09090b"
            />
            <path
              d="M25 75V25H55C66.0457 25 75 33.9543 75 45C75 56.0457 66.0457 65 55 65H25"
              stroke="#ffffff"
              strokeWidth="6"
              strokeLinecap="square"
            />
            <path
              d="M50 65L75 75"
              stroke="#00f3ff"
              strokeWidth="6"
              strokeLinecap="square"
            />
            <circle cx="50" cy="45" r="4" fill="#00f3ff" />
          </svg>
        </LogoWrapper>
        <BrandName>
          <BrandTitle>RAQIM</BrandTitle>
          <BrandSubtitle>Console // Core</BrandSubtitle>
        </BrandName>
      </LogoSection>

      <ProfileSection>
        <ProfileHeader>Local Control Plane</ProfileHeader>
        <OperatorDetails>
          <TerminalAvatar $isActive={daemonOnline}>
            <AvatarText $isActive={daemonOnline}>
              {daemonOnline ? 'OK' : 'OFF'}
            </AvatarText>
          </TerminalAvatar>
          <OperatorMeta>
            <OperatorId>
              {clusterInfo?.node_id ? `NODE: ${clusterInfo.node_id.slice(0, 10)}` : 'DEV_ENGINE_01'}
            </OperatorId>
            <OperatorStatus>
              <HeartbeatDot $isActive={daemonOnline} />
              <StatusText $isActive={daemonOnline}>
                {daemonOnline ? 'LIVE DAEMON' : 'DISCONNECTED'}
              </StatusText>
            </OperatorStatus>
          </OperatorMeta>
        </OperatorDetails>
      </ProfileSection>

      <NavList>
        {navLinks.map((link) => {
          const isActive =
            pathname === link.href ||
            (link.href === '/aegis' && pathname === '/firewall') ||
            (link.href === '/vault' && pathname === '/audit-vault') ||
            (link.href === '/replay' && pathname === '/router');
          const Icon = link.icon;

          return (
            <NavLink key={link.href} href={link.href} $isActive={isActive}>
              {isActive && (
                <ActiveLine
                  layoutId="activeNavLine"
                  transition={{ type: 'spring', stiffness: 400, damping: 30 }}
                />
              )}
              <Icon className={`w-4 h-4 ${isActive ? 'text-cyan-400' : 'text-zinc-500'}`} />
              <span>{link.label}</span>
            </NavLink>
          );
        })}
      </NavList>

      <BottomSection>
        <DiagnosticGroup>
          <DiagnosticRow>
            <DiagnosticLabel>DAEMON</DiagnosticLabel>
            <DiagnosticValue $accent={daemonOnline} $alert={!daemonOnline}>
              {daemonOnline ? 'CONNECTED' : 'DISCONNECTED'}
            </DiagnosticValue>
          </DiagnosticRow>
          <DiagnosticRow>
            <DiagnosticLabel>THROUGHPUT</DiagnosticLabel>
            <DiagnosticValue>{currentTps} TPS</DiagnosticValue>
          </DiagnosticRow>
          <DiagnosticRow>
            <DiagnosticLabel>SHARDS</DiagnosticLabel>
            <DiagnosticValue>{activeTopology.length} ACTIVE</DiagnosticValue>
          </DiagnosticRow>
          <DiagnosticRow>
            <DiagnosticLabel>QUARANTINE</DiagnosticLabel>
            <DiagnosticValue $alert={quarantinedAgents.length > 0}>
              {quarantinedAgents.length} BLOCKED
            </DiagnosticValue>
          </DiagnosticRow>
        </DiagnosticGroup>
      </BottomSection>
    </SidebarContainer>
  );
}
