// Visual prototype only. No capture, key loading, PCAP reading, or network I/O.
function $(id: "packet-search"): HTMLInputElement;
function $(id: "status-filter"): HTMLSelectElement;
function $(id: "help-dialog"): HTMLDialogElement;
function $(id: string): HTMLElement;
function $(id: string): HTMLElement {
  const element = document.getElementById(id);
  if (!element) throw new Error("Missing UI element: " + id);
  return element;
}
const server = "192.168.10.157";
const client = "192.168.10.180";

type PacketStatus = "authenticated" | "proof_verified" | "authentication_failed";
type PacketTemplate = {
  kind: string;
  protocol: string;
  direction: string;
  source: string;
  destination: string;
  status: PacketStatus;
  hex: string;
};
type Packet = PacketTemplate & { id: number; time: string };
type ActivityEvent = { title: string; detail: string; tone: string; time: string };
type View = "overview" | "packets";

const templates: PacketTemplate[] = [
  { kind: "RTTProtocol · Type 0", protocol: "RTTProtocol", direction: "S → C", source: server, destination: client, status: "authenticated", hex: "00 00 00 00 00 00 0b 71 c4 00 00 00 00 00 01 4c 20 00 00 00 00" },
  { kind: "RTTProtocol · Type 1", protocol: "RTTProtocol", direction: "C → S", source: client, destination: server, status: "authenticated", hex: "01 00 00 00 00 00 0b 71 c4 00 00 00 00 00 01 4c 20 00 00 00 01" },
  { kind: "CoopSequenceNetState", protocol: "Unreliable", direction: "S → C", source: server, destination: client, status: "authenticated", hex: "01 7e 2d 00 00 00 a2 08 03 12 44 00 00 00 00 00" },
  { kind: "PrivateSetting", protocol: "BroadcastReliable", direction: "S → C", source: server, destination: client, status: "authenticated", hex: "07 00 46 00 0d 00 0d 00 1a 00 00 00 38 f4 06 f7 35 85 16 b6 56 56 07 07 43 6f 70 5f 53 68 61 6b 65 73 70 69 72 61 6c 00 00 00 00 00 00 00 00" },
  { kind: "Net 0x32", protocol: "NetProtocol", direction: "C → S", source: client, destination: server, status: "authenticated", hex: "01 32 00 00 00 00 00 00 12 34 56 78 9a bc de f0" },
  { kind: "BrowseReply", protocol: "Browse", direction: "S → C", source: server, destination: client, status: "proof_verified", hex: "01 00 00 02 6c 0a 1d 2c 44 15 00 00 00 00 00 00 00 01 20 00 00 00 00 00" },
  { kind: "Pia frame", protocol: "Pia", direction: "C → S", source: client, destination: server, status: "authentication_failed", hex: "32 ab 98 64 8f 00 00 00 00 00 00 00 01 02 03 04 05 06 07 08 09 0a 0b 0c" },
];

let packets: Packet[] = [];
let nextRecord = 480;
let selectedId: number | null = null;
let activeView: View = "overview";
let paused = false;
let tick = 0;
const activity: ActivityEvent[] = [
  { title: "WAVE 02 state updated", detail: "Sequence 1 · Quota field 24", tone: "amber", time: "NOW" },
  { title: "RTT response paired", detail: "Client ↔ Server · 42.6 ms (sample)", tone: "", time: "2s ago" },
  { title: "PrivateSetting received", detail: "Cop_Shakespiral · Difficulty 1665", tone: "blue", time: "12s ago" },
  { title: "Pia packet authenticated", detail: "PiaKey derived from BrowseReply", tone: "", time: "18s ago" },
  { title: "Room discovered", detail: "Server 192.168.10.157", tone: "blue", time: "24s ago" },
];

function clockText() {
  return new Intl.DateTimeFormat("en-GB", {
    timeZone: "Asia/Tokyo", hour: "2-digit", minute: "2-digit", second: "2-digit",
    hour12: false,
  }).format(new Date()) + " JST";
}

function addPacket(template: PacketTemplate, render = true): void {
  const packet: Packet = {
    ...template,
    id: nextRecord++,
    time: new Intl.DateTimeFormat("en-GB", {
      timeZone: "Asia/Tokyo", hour: "2-digit", minute: "2-digit", second: "2-digit",
      hour12: false,
    }).format(new Date()),
  };
  packets.unshift(packet);
  if (packets.length > 60) packets.pop();
  if (selectedId === null) selectedId = packet.id;
  if (render) renderAll();
}

function setView(view: View): void {
  activeView = view;
  for (const name of ["overview", "packets"]) {
    const selected = name === view;
    const tab = $("tab-" + name);
    const panel = $("view-" + name);
    tab.classList.toggle("active", selected);
    tab.setAttribute("aria-selected", String(selected));
    panel.classList.toggle("active", selected);
    panel.hidden = !selected;
  }
  if (view === "packets") renderPackets();
}

function statusClass(status: PacketStatus): string {
  return status === "authentication_failed" ? "fail" : "ok";
}

function visiblePackets(): Packet[] {
  const query = $("packet-search").value.trim().toLowerCase();
  const filter = $("status-filter").value;
  return packets.filter((packet) => {
    if (filter === "authenticated" && packet.status === "authentication_failed") return false;
    if (filter === "failed" && packet.status !== "authentication_failed") return false;
    return !query || [packet.kind, packet.protocol, packet.source, packet.destination, packet.status, packet.direction, String(packet.id)]
      .some((value) => value.toLowerCase().includes(query));
  });
}

function selectPacket(id: number): void {
  selectedId = id;
  renderPackets();
  const row = document.querySelector('.packet-table tr[data-id="' + id + '"]');
  if (row) row.scrollIntoView({ block: "nearest" });
}

function renderPackets(): void {
  const shown = visiblePackets();
  $("visible-count").textContent = shown.length + " shown";
  const body = $("packet-rows");
  body.replaceChildren();
  for (const packet of shown) {
    const tr = document.createElement("tr");
    tr.dataset.id = String(packet.id);
    tr.tabIndex = 0;
    tr.classList.toggle("selected", packet.id === selectedId);
    const values = [
      "#" + packet.id,
      packet.time,
      packet.direction,
      packet.kind,
      packet.status === "authentication_failed" ? "FAILED" : "AUTHENTICATED",
    ];
    values.forEach((value, index) => {
      const cell = document.createElement("td");
      if (index === 2) cell.className = "flow-arrow";
      if (index === 3) cell.className = "kind-name";
      if (index === 4) {
        const badge = document.createElement("span");
        badge.className = "status-badge " + statusClass(packet.status);
        badge.textContent = value;
        cell.append(badge);
      } else {
        cell.textContent = value;
      }
      tr.append(cell);
    });
    tr.addEventListener("click", () => selectPacket(packet.id));
    tr.addEventListener("keydown", (event) => {
      if (event.key === "Enter") selectPacket(packet.id);
    });
    body.append(tr);
  }
  renderDetail();
}

function bytesOf(packet: Packet): number[] {
  return packet.hex.trim().split(/\s+/).map((part) => Number.parseInt(part, 16));
}

function formatHex(bytes: number[]): string {
  const lines: string[] = [];
  for (let offset = 0; offset < bytes.length; offset += 16) {
    const row = bytes.slice(offset, offset + 16);
    const hex = row.map((byte) => byte.toString(16).padStart(2, "0")).join(" ");
    const ascii = row.map((byte) => byte >= 32 && byte <= 126 ? String.fromCharCode(byte) : ".").join("");
    lines.push(offset.toString(16).padStart(4, "0") + "  " + hex.padEnd(47, " ") + "  |" + ascii + "|");
  }
  return lines.join("\n");
}

function renderDetail(): void {
  const packet = packets.find((item) => item.id === selectedId);
  if (!packet) {
    $("detail-title").textContent = "Select a packet";
    $("detail-meta").replaceChildren();
    $("hex-view").textContent = "";
    $("hex-size").textContent = "0 bytes";
    return;
  }
  $("detail-title").textContent = "#" + packet.id + " · " + packet.kind;
  const meta = $("detail-meta");
  meta.replaceChildren();
  for (const [label, value] of [
    ["Time", packet.time + " JST"],
    ["Source", packet.source + ":49152"],
    ["Destination", packet.destination + ":49152"],
    ["Direction", packet.direction],
    ["Protocol", packet.protocol],
    ["Status", packet.status],
  ]) {
    const dt = document.createElement("dt");
    const dd = document.createElement("dd");
    dt.textContent = label;
    dd.textContent = value;
    meta.append(dt, dd);
  }
  const bytes = bytesOf(packet);
  $("hex-size").textContent = bytes.length + " bytes";
  $("hex-view").textContent = formatHex(bytes);
}

function renderActivity(): void {
  const list = $("activity-list");
  list.replaceChildren();
  for (const event of activity.slice(0, 5)) {
    const li = document.createElement("li");
    const dot = document.createElement("span");
    dot.className = "activity-dot " + event.tone;
    const text = document.createElement("div");
    text.className = "activity-text";
    const title = document.createElement("strong");
    const detail = document.createElement("small");
    title.textContent = event.title;
    detail.textContent = event.detail;
    text.append(title, detail);
    const time = document.createElement("span");
    time.className = "activity-time";
    time.textContent = event.time;
    li.append(dot, text, time);
    list.append(li);
  }
}

function renderTotals(): void {
  // These counters count generated demo records, including records outside the visible ring.
  const total = nextRecord - 480;
  const authenticated = total - Math.floor(total / templates.length);
  $("packet-total").textContent = total.toLocaleString("en-US");
  $("packet-total-side").textContent = total.toLocaleString("en-US");
  $("auth-total").textContent = authenticated.toLocaleString("en-US");
  $("auth-total-side").textContent = authenticated.toLocaleString("en-US");
}

function renderAll(): void {
  renderTotals();
  renderActivity();
  if (activeView === "packets") renderPackets();
}

function selectWave(number: 1 | 2 | 3): void {
  document.querySelectorAll<HTMLButtonElement>(".wave-track").forEach((row) => row.classList.toggle("active", Number(row.dataset.wave) === number));
  const descriptions: Record<1 | 2 | 3, [string, string]> = {
    1: ["Low tide", "Quota 21 · Event 0"],
    2: ["Normal tide", "Quota 24 · Event 5"],
    3: ["High tide", "Quota 26 · Event 0"],
  };
  const detail = $("wave-detail");
  detail.replaceChildren();
  for (const value of ["WAVE 0" + number, ...descriptions[number]]) {
    const element = document.createElement(value === descriptions[number][0] ? "strong" : "span");
    element.textContent = value;
    detail.append(element);
  }
}

function togglePause(): void {
  paused = !paused;
  $("pause-button").innerHTML = paused
    ? '<span class="pause-icon">▶</span> Resume simulation'
    : '<span class="pause-icon">Ⅱ</span> Pause simulation';
  const captureLabel = document.querySelector(".capture-state")?.lastChild;
  if (captureLabel) captureLabel.textContent = paused ? " SIMULATION PAUSED" : " SIMULATED CAPTURE";
}

function simulate(): void {
  if (paused) return;
  tick += 1;
  const serverRtt = (38.2 + Math.sin(tick / 3) * 3.4).toFixed(1);
  const clientRtt = (42.6 + Math.cos(tick / 4) * 4.1).toFixed(1);
  $("server-rtt").textContent = serverRtt;
  $("client-rtt").textContent = clientRtt;
  $("metric-rtt").textContent = clientRtt;
  addPacket(templates[tick % templates.length]);
  if (tick % 7 === 0) {
    activity.unshift({ title: "RTT response paired", detail: "Client ↔ Server · " + clientRtt + " ms (sample)", tone: "", time: "NOW" });
    activity.length = Math.min(activity.length, 5);
    renderActivity();
  }
}

document.querySelectorAll<HTMLButtonElement>(".view-tab").forEach((button) => button.addEventListener("click", () => setView(button.dataset.view === "packets" ? "packets" : "overview")));
document.querySelectorAll<HTMLButtonElement>(".wave-track").forEach((button) => button.addEventListener("click", () => {
  const number = Number(button.dataset.wave);
  if (number === 1 || number === 2 || number === 3) selectWave(number);
}));
$("pause-button").addEventListener("click", togglePause);
$("packet-search").addEventListener("input", renderPackets);
$("status-filter").addEventListener("change", renderPackets);
$("help-button").addEventListener("click", () => $("help-dialog").showModal());
$("close-help").addEventListener("click", () => $("help-dialog").close());

document.addEventListener("keydown", (event) => {
  const typing = event.target instanceof HTMLInputElement || event.target instanceof HTMLSelectElement;
  if (event.key === "Escape" && $("help-dialog").open) { $("help-dialog").close(); return; }
  if ($("help-dialog").open) return;
  if (typing) return;
  if (event.key === "1") setView("overview");
  if (event.key === "2") setView("packets");
  if (event.key === "Tab" && event.target === document.body) {
    event.preventDefault();
    setView(activeView === "overview" ? "packets" : "overview");
  }
  if (event.key === " " && !(event.target instanceof HTMLButtonElement)) {
    event.preventDefault();
    togglePause();
  }
  if (event.key === "/" && activeView === "packets") {
    event.preventDefault();
    $("packet-search").focus();
  }
  if (activeView === "packets" && ["ArrowUp", "ArrowDown"].includes(event.key)) {
    const shown = visiblePackets();
    const index = shown.findIndex((item) => item.id === selectedId);
    const next = Math.max(0, Math.min(shown.length - 1, index + (event.key === "ArrowDown" ? 1 : -1)));
    if (shown[next]) { event.preventDefault(); selectPacket(shown[next].id); }
  }
});

for (let index = 0; index < 8; index += 1) addPacket(templates[index % templates.length], false);
selectedId = packets[0].id;
renderAll();
setView("overview");
$("clock").textContent = clockText();
setInterval(() => { $("clock").textContent = clockText(); }, 1000);
setInterval(simulate, 1800);
