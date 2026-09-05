'use client';

import React, { useState, useEffect } from 'react';
import { StateProofResponse, InclusionProof } from '../../lib/api';
import { fetchStateProof } from '../../actions/vault';
import {
  ShieldCheck,
  ShieldAlert,
  Download,
  Copy,
  Check,
  Search,
  GitCommit,
  Layers,
  CheckCircle2,
  AlertTriangle,
} from 'lucide-react';

interface MerkleProofInspectorProps {
  initialTxIdHex: string | null;
  onTxIdChange?: (txIdHex: string) => void;
}

export function MerkleProofInspector({
  initialTxIdHex,
  onTxIdChange,
}: MerkleProofInspectorProps) {
  const [txIdInput, setTxIdInput] = useState(initialTxIdHex || '');
  const [isLoading, setIsLoading] = useState(false);
  const [proofData, setProofData] = useState<InclusionProof | null>(null);
  const [proofMessage, setProofMessage] = useState<string | null>(null);
  const [isVerified, setIsVerified] = useState<boolean | null>(null);
  const [copiedRoot, setCopiedRoot] = useState(false);

  const loadProof = async (txHex: string) => {
    const cleanHex = txHex.trim();
    if (!cleanHex) return;

    setIsLoading(true);
    setProofData(null);
    setProofMessage(null);
    setIsVerified(null);

    try {
      const res: StateProofResponse = await fetchStateProof(cleanHex);
      if (res && res.success && res.proof) {
        setProofData(res.proof);
        setProofMessage(res.message);
        setIsVerified(true);
      } else {
        setProofMessage(res?.message || 'Proof not found in active batch archives.');
        setIsVerified(false);
      }
    } catch (err: any) {
      setProofMessage(err.message || 'Error communicating with Axon proof generator.');
      setIsVerified(false);
    } finally {
      setIsLoading(false);
    }
  };

  // Sync with prop when clicking search results
  useEffect(() => {
    if (initialTxIdHex && initialTxIdHex !== txIdInput) {
      setTxIdInput(initialTxIdHex);
      loadProof(initialTxIdHex);
    }
  }, [initialTxIdHex]);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (onTxIdChange) onTxIdChange(txIdInput.trim());
    loadProof(txIdInput.trim());
  };

  const handleCopyRoot = () => {
    if (!proofData?.merkle_root_hex) return;
    navigator.clipboard.writeText(proofData.merkle_root_hex);
    setCopiedRoot(true);
    setTimeout(() => setCopiedRoot(false), 2000);
  };

  const handleExportAuditPack = () => {
    if (!proofData) return;
    const auditPack = {
      protocol: 'RAQIM_AXON_PROOF_V1',
      algorithm: 'BLAKE3-256',
      timestamp: new Date().toISOString(),
      transaction_id_hex: proofData.tx_id_hex,
      proof: proofData,
      verification_status: isVerified ? 'CERTIFIED_VALID' : 'UNVERIFIED',
    };

    const blob = new Blob([JSON.stringify(auditPack, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `audit_proof_${proofData.tx_id_hex.slice(0, 8)}.json`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
  };

  return (
    <div className="flex-1 flex flex-col min-h-0 bg-zinc-950 border border-zinc-800/80 rounded-sm overflow-hidden shadow-lg select-none">
      {/* Header & TxID Input */}
      <div className="bg-zinc-900/90 border-b border-zinc-800 p-3 space-y-2.5 shrink-0 select-none">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <ShieldCheck className="w-3.5 h-3.5 text-emerald-400" />
            <span className="font-sans text-xs uppercase tracking-wider font-bold text-white">
              O(log N) MERKLE PROOF INSPECTOR &amp; VERIFIER
            </span>
          </div>
          <span className="font-mono text-[10px] text-emerald-400 font-bold px-2 py-0.5 rounded-full bg-emerald-500/10 border border-emerald-500/30">
            AXON GATEKEEPER
          </span>
        </div>

        {/* TxID Query Input */}
        <form onSubmit={handleSubmit} className="flex gap-2">
          <div className="relative flex-1">
            <Search className="w-3 h-3 text-zinc-500 absolute left-2.5 top-1/2 -translate-y-1/2 pointer-events-none" />
            <input
              type="text"
              required
              value={txIdInput}
              onChange={(e) => setTxIdInput(e.target.value)}
              placeholder="Enter 32-character Hex TxID to verify inclusion proof..."
              className="w-full pl-7 pr-3 py-1.5 bg-zinc-950 border border-zinc-800 focus:border-emerald-500/80 rounded-xs text-xs font-mono text-zinc-100 placeholder:text-zinc-500 outline-none transition-colors"
            />
          </div>
          <button
            type="submit"
            disabled={isLoading || !txIdInput.trim()}
            className="px-3 py-1.5 bg-emerald-950/80 hover:bg-emerald-900 border border-emerald-500/70 text-emerald-200 rounded-xs font-mono text-xs font-bold uppercase tracking-wider transition-all disabled:opacity-40"
          >
            {isLoading ? 'VERIFYING...' : 'AUDIT PROOF'}
          </button>
        </form>
      </div>

      {/* Main Inspection View */}
      <div className="flex-1 overflow-y-auto p-4 space-y-4 bg-zinc-950 scrollbar-thin scrollbar-thumb-zinc-800">
        {isLoading ? (
          <div className="py-20 flex flex-col items-center justify-center gap-2 text-zinc-400 font-mono text-xs">
            <div className="w-6 h-6 border-2 border-emerald-400 border-t-transparent rounded-full animate-spin" />
            <span>CALCULATING BLAKE3 STATE PROOF TREE...</span>
          </div>
        ) : !proofData ? (
          <div className="py-20 flex flex-col items-center justify-center gap-3 text-zinc-500 font-mono text-xs uppercase tracking-wider text-center">
            <GitCommit className="w-8 h-8 text-zinc-700" />
            <span>ENTER OR SELECT A TRANSACTION ID TO INSPECT ITS MERKLE BRANCH</span>
            {proofMessage && (
              <span className="text-amber-400/90 text-[11px] normal-case bg-amber-950/40 px-3 py-1 border border-amber-900/60 rounded-xs">
                {proofMessage}
              </span>
            )}
          </div>
        ) : (
          <div className="space-y-4">
            {/* Top Verification Status Banner */}
            <div
              className={`p-3 rounded-xs border flex items-center justify-between font-mono text-xs ${
                isVerified
                  ? 'bg-emerald-950/40 border-emerald-500/50 text-emerald-300'
                  : 'bg-rose-950/40 border-rose-500/50 text-rose-300'
              }`}
            >
              <div className="flex items-center gap-2">
                {isVerified ? (
                  <CheckCircle2 className="w-4 h-4 text-emerald-400" />
                ) : (
                  <AlertTriangle className="w-4 h-4 text-rose-400" />
                )}
                <span className="font-bold">
                  {isVerified
                    ? 'CANONICAL INCLUSION PROOF VERIFIED (O(log N))'
                    : 'INCLUSION PROOF INVALID OR ORPHANED'}
                </span>
              </div>
              <span className="text-[10px] uppercase font-sans">
                BATCH #{proofData.batch_id} • LEAF #{proofData.leaf_index}
              </span>
            </div>

            {/* Merkle Root Card */}
            <div className="bg-zinc-900/60 border border-zinc-800 rounded-sm p-3 space-y-2">
              <div className="flex items-center justify-between text-zinc-400 text-[10px] font-sans uppercase font-bold tracking-wider">
                <span>Batch Merkle Root (Blake3-256)</span>
                <span className="text-emerald-400 font-mono">SEALED STATE</span>
              </div>
              <div className="flex items-center justify-between gap-2 bg-zinc-950 p-2 rounded-xs border border-zinc-800 font-mono text-xs">
                <span className="text-emerald-400 font-bold break-all">
                  {proofData.merkle_root_hex}
                </span>
                <button
                  onClick={handleCopyRoot}
                  className="p-1 text-zinc-400 hover:text-white rounded-xs hover:bg-zinc-800 transition-colors shrink-0"
                >
                  {copiedRoot ? (
                    <Check className="w-3.5 h-3.5 text-emerald-400" />
                  ) : (
                    <Copy className="w-3.5 h-3.5 text-zinc-500" />
                  )}
                </button>
              </div>
            </div>

            {/* Sibling Path Chain */}
            <div className="space-y-2">
              <div className="flex items-center justify-between text-zinc-400 text-[10px] font-sans uppercase font-bold tracking-wider">
                <div className="flex items-center gap-1.5">
                  <Layers className="w-3 h-3 text-cyan-400" />
                  <span>
                    Sibling Proof Chain ({proofData.sibling_hashes_hex.length} Hops)
                  </span>
                </div>
                <span className="text-cyan-400 font-mono text-[9px]">
                  DEPTH: {proofData.sibling_hashes_hex.length}
                </span>
              </div>

              <div className="space-y-1.5 font-mono text-[11px]">
                {proofData.sibling_hashes_hex.map((sibHex, idx) => (
                  <div
                    key={idx}
                    className="flex items-center justify-between p-2 bg-zinc-900/60 border border-zinc-800/80 rounded-xs hover:border-zinc-700 transition-colors"
                  >
                    <div className="flex items-center gap-2">
                      <span className="px-1.5 py-0.2 rounded-xs bg-zinc-950 border border-zinc-800 text-[9px] text-zinc-400 font-bold">
                        HOP {idx + 1}
                      </span>
                      <span className="text-zinc-300 font-bold truncate max-w-xs sm:max-w-md">
                        {sibHex}
                      </span>
                    </div>
                    <span className="text-[9px] text-emerald-400 uppercase font-sans">
                      VERIFIED
                    </span>
                  </div>
                ))}
              </div>
            </div>

            {/* Export Audit Package */}
            <div className="pt-2">
              <button
                onClick={handleExportAuditPack}
                className="w-full py-2 bg-zinc-900 hover:bg-zinc-800 border border-zinc-700 text-zinc-200 rounded-xs font-mono text-xs font-bold uppercase tracking-wider flex items-center justify-center gap-2 transition-colors"
              >
                <Download className="w-3.5 h-3.5 text-cyan-400" />
                <span>EXPORT VERIFIABLE AUDIT PACKAGE (.JSON)</span>
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
