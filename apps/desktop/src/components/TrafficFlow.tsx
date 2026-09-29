import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { desktop } from "../api/desktop";
import type { ClientInstance } from "../api/models";
import { clientColor } from "../lib/outbound";
import { presetById, type PresetId } from "../lib/presets";
import { useAppStore } from "../store/app";
import { StatusPill } from "./StatusPill";
import { InfoTip } from "./ui/InfoTip";

const FLOW_WIDTH = 760;
const FLOW_PACKET_LIFE_MS = 4_200;
const FLOW_PACKET_STAGGER_MS = 900;
const FLOW_MAX_PACKETS = 6;

type FlowBranch = {
  key: string;
  label: string;
  /** Concrete accent color; DIRECT is green, every client gets its own. */
  color: string;
  y: number;
  d: string;
  isDefault: boolean;
  client?: ClientInstance;
};

type FlowPacket = {
  id: string;
  label: string;
  branchKey: string;
  born: number;
  /** Alternates labels above/below the dot so they never overlap. */
  lane: 1 | -1;
};

export function TrafficFlow() {
  const { t } = useTranslation();
  const settings = useAppStore((state) => state.settings);
  const snapshot = useAppStore((state) => state.snapshot);
  const clients = settings?.clients;
  const live = snapshot?.live_route;
  const liveKnown = Boolean(live?.match_proxy);
  const defaultClientId = liveKnown
    ? (live?.match_client_id ?? null)
    : settings?.default_route.kind === "client"
      ? settings.default_route.client_id
      : null;
  const defaultIsDirect = liveKnown
    ? live?.match_proxy === "DIRECT"
    : settings?.default_route.kind === "direct";
  const [selected, setSelected] = useState<string | null>(null);
  const [packets, setPackets] = useState<FlowPacket[]>([]);
  const recentPackets = useRef<Map<string, { branch: string; at: number }>>(
    new Map(),
  );
  const laneFlip = useRef<1 | -1>(1);
  const pathRefs = useRef<Map<string, SVGPathElement>>(new Map());
  const packetRefs = useRef<Map<string, SVGGElement>>(new Map());
  const packetsRef = useRef<FlowPacket[]>([]);
  packetsRef.current = packets;

  // One branch for DIRECT plus one per enabled client; geometry grows with
  // the branch count so the diagram stays readable as clients are added.
  const { branches, height, deviceY } = useMemo(() => {
    const enabled = (clients ?? []).filter((client) => client.enabled);
    const rows: Array<Omit<FlowBranch, "y" | "d">> = [
      {
        key: "direct",
        label: t("direct"),
        color: "rgb(34 197 94)",
        isDefault: defaultIsDirect,
      },
      ...enabled.map((client) => ({
        key: client.id,
        label: presetById(client.preset as PresetId).title,
        color: clientColor(client.id, clients ?? []),
        isDefault: client.id === defaultClientId,
        client,
      })),
    ];
    const gap = 96;
    const top = 58;
    const svgHeight = Math.max(top + (rows.length - 1) * gap + 110, 230);
    const centerY = svgHeight / 2 - 6;
    const placed = rows.map((row, index) => {
      const y = top + index * gap;
      return {
        ...row,
        y,
        d: `M96 ${centerY} H292 C390 ${centerY} 410 ${y} 520 ${y} H660`,
      };
    });
    return { branches: placed, height: svgHeight, deviceY: centerY };
  }, [clients, defaultClientId, defaultIsDirect, t]);

  // Live packets: poll the active connections and float each new host along
  // its real route so the user sees which domain uses which client.
  // A stable signature keeps the polling effect from restarting on every
  // render (branch objects are rebuilt whenever settings re-memoize).
  useEffect(() => {
    recentPackets.current.clear();
    setPackets([]);
  }, [defaultClientId]);

  const branchSignature = branches.map((branch) => branch.key).join("|");
  useEffect(() => {
    const media = window.matchMedia("(prefers-reduced-motion: reduce)");
    let stopped = false;
    const timers: number[] = [];
    const branchKeys = new Set(branchSignature.split("|"));
    const branchForOutbound = (outbound: string): string | null => {
      const value = outbound.toLowerCase();
      if (value === "direct") return "direct";
      const id = value.replace(/^(client|proxy)-/u, "");
      return branchKeys.has(id) ? id : null;
    };
    const tick = async () => {
      try {
        const rows = await desktop.listActiveConnections();
        if (stopped || media.matches) return;
        setPackets((previous) => {
          const next = [...previous];
          let added = false;
          let stagger = 0;
          for (const row of rows) {
            if (next.length >= FLOW_MAX_PACKETS) break;
            const branchKey = branchForOutbound(row.outbound);
            if (!branchKey) continue;
            const label = row.host || row.destination_ip;
            if (!label) continue;
            const lastSeen = recentPackets.current.get(label);
            const sameBranch = lastSeen?.branch === branchKey;
            if (
              sameBranch &&
              Date.now() - (lastSeen?.at ?? 0) < FLOW_PACKET_LIFE_MS * 2
            ) {
              continue;
            }
            recentPackets.current.set(label, {
              branch: branchKey,
              at: Date.now(),
            });
            added = true;
            laneFlip.current = laneFlip.current === 1 ? -1 : 1;
            const shown = label.length > 22 ? `${label.slice(0, 21)}…` : label;
            for (let index = next.length - 1; index >= 0; index -= 1) {
              const packet = next[index];
              if (packet?.label === shown && packet.branchKey !== branchKey) {
                next.splice(index, 1);
              }
            }
            const id = `${label}|${branchKey}|${Date.now()}`;
            next.push({
              id,
              label: shown,
              branchKey,
              // Staggered births keep simultaneous packets apart on the path.
              born: Date.now() + stagger,
              lane: laneFlip.current,
            });
            timers.push(
              window.setTimeout(() => {
                setPackets((current) =>
                  current.filter((packet) => packet.id !== id),
                );
              }, FLOW_PACKET_LIFE_MS + stagger),
            );
            stagger += FLOW_PACKET_STAGGER_MS;
          }
          return added ? next : previous;
        });
      } catch {
        // The stack may be tearing down between polls; skip this round.
      }
    };
    void tick();
    const interval = window.setInterval(() => void tick(), 1_000);
    return () => {
      stopped = true;
      window.clearInterval(interval);
      for (const timer of timers) window.clearTimeout(timer);
    };
  }, [branchSignature]);

  // SMIL animateMotion does not start reliably for dynamically inserted
  // nodes in the embedded webview, so packets are moved by hand along the
  // measured path every animation frame.
  const hasPackets = packets.length > 0;
  useEffect(() => {
    if (!hasPackets) return;
    let frame = 0;
    const step = () => {
      const now = Date.now();
      for (const packet of packetsRef.current) {
        const node = packetRefs.current.get(packet.id);
        const path = pathRefs.current.get(packet.branchKey);
        if (!node || !path) continue;
        const progress = Math.min(
          (now - packet.born) / (FLOW_PACKET_LIFE_MS - 200),
          1,
        );
        if (progress < 0) {
          node.setAttribute("opacity", "0");
          continue;
        }
        const point = path.getPointAtLength(progress * path.getTotalLength());
        node.setAttribute("transform", `translate(${point.x} ${point.y})`);
        // Soft fade at both ends instead of popping in and out.
        const fade =
          progress < 0.15
            ? progress / 0.15
            : progress > 0.82
              ? Math.max((1 - progress) / 0.18, 0)
              : 1;
        node.setAttribute("opacity", fade.toFixed(3));
      }
      frame = window.requestAnimationFrame(step);
    };
    frame = window.requestAnimationFrame(step);
    return () => window.cancelAnimationFrame(frame);
  }, [hasPackets]);

  const selectedBranch = branches.find((branch) => branch.key === selected);
  const selectedClientStatus = selectedBranch?.client
    ? snapshot?.clients.find((item) => item.id === selectedBranch.key)
    : null;
  const selectedStatus = selectedClientStatus?.status ?? null;
  const directIp = useAppStore((state) => state.networkStatus?.public_ip);

  return (
    <section className="ui-enter h-full overflow-x-hidden rounded-2xl border border-ink/10 bg-surface p-3.5">
      <div className="flex items-center gap-1.5">
        <h2 className="font-semibold">{t("liveRouting")}</h2>
        <InfoTip text={t("liveRoutingHelp")} />
      </div>
      <div className="relative">
        <svg
          data-testid="live-routing"
          className="mt-2 h-auto w-full"
          viewBox={`0 0 ${FLOW_WIDTH} ${height}`}
          role="img"
          aria-label={t("liveRoutingAria")}
        >
          {branches.map((branch) => (
            <g key={`route-${branch.key}`}>
              <path
                className="traffic-flow-base"
                d={branch.d}
                ref={(node) => {
                  if (node) pathRefs.current.set(branch.key, node);
                  else pathRefs.current.delete(branch.key);
                }}
              />
              <path
                className="traffic-flow-route"
                style={{ stroke: branch.color }}
                d={branch.d}
              />
            </g>
          ))}

          <circle cx="76" cy={deviceY} r="30" fill="rgb(var(--brand) / 0.12)" />
          <circle cx="76" cy={deviceY} r="9" fill="rgb(var(--brand))" />
          <text
            className="traffic-flow-label"
            x="76"
            y={deviceY + 48}
            textAnchor="middle"
          >
            {t("device")}
          </text>

          {branches.map((branch) => (
            <g
              key={`node-${branch.key}`}
              role="button"
              tabIndex={0}
              aria-label={`${branch.label} status`}
              className="cursor-pointer focus:outline-none"
              onClick={() =>
                setSelected((current) =>
                  current === branch.key ? null : branch.key,
                )
              }
              onKeyDown={(event) => {
                if (event.key === "Enter" || event.key === " ") {
                  event.preventDefault();
                  setSelected((current) =>
                    current === branch.key ? null : branch.key,
                  );
                }
              }}
            >
              <circle
                cx="686"
                cy={branch.y}
                r="27"
                fill={branch.color}
                opacity="0.14"
              />
              <circle cx="686" cy={branch.y} r="8" fill={branch.color} />
              <text
                className="traffic-flow-label"
                x="686"
                y={branch.y + 42}
                textAnchor="middle"
              >
                {branch.label}
              </text>
              {branch.isDefault ? (
                <text
                  className="traffic-flow-default"
                  x="686"
                  y={branch.y + 58}
                  textAnchor="middle"
                >
                  {t("matchDefault")}
                </text>
              ) : null}
            </g>
          ))}

          {packets.map((packet) => {
            const branch = branches.find(
              (item) => item.key === packet.branchKey,
            );
            if (!branch) return null;
            return (
              <g
                key={packet.id}
                className="traffic-flow-packet"
                ref={(node) => {
                  if (node) packetRefs.current.set(packet.id, node);
                  else packetRefs.current.delete(packet.id);
                }}
              >
                <circle
                  r="6"
                  fill={branch.color}
                  stroke="rgb(var(--surface))"
                  strokeWidth="1.5"
                />
                <text
                  className="traffic-flow-packet-label"
                  y={packet.lane === 1 ? -12 : 23}
                  textAnchor="middle"
                >
                  {packet.label}
                </text>
              </g>
            );
          })}
        </svg>

        {selectedBranch ? (
          <div
            role="status"
            className="absolute end-1 z-10 max-w-56 rounded-xl border border-ink/15 bg-canvas p-3 text-xs shadow-card"
            style={{ top: `${(selectedBranch.y / height) * 100}%` }}
          >
            <p className="font-semibold">{selectedBranch.label}</p>
            {selectedBranch.client ? (
              <>
                <div className="mt-1">
                  <StatusPill phase={selectedStatus?.phase ?? "stopped"} />
                </div>
                {selectedStatus?.message ? (
                  <p className="mt-1 text-muted">{selectedStatus.message}</p>
                ) : null}
                {selectedClientStatus?.exit_ip ? (
                  <p className="mt-1 font-mono">
                    {t("exitIp")}: {selectedClientStatus.exit_ip}
                  </p>
                ) : null}
              </>
            ) : (
              <>
                <p className="mt-1 text-muted">{t("directTooltip")}</p>
                {directIp ? (
                  <p className="mt-1 font-mono">
                    {t("exitIp")}: {directIp}
                  </p>
                ) : null}
              </>
            )}
          </div>
        ) : null}
      </div>
    </section>
  );
}
