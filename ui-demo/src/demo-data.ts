export type PacketStatus = "authenticated" | "proof_verified" | "authentication_failed"

export type Packet = {
  id: number
  time: string
  flow: "S → C" | "C → S"
  source: string
  destination: string
  protocol: string
  kind: string
  status: PacketStatus
  rawHex: string
  fields: Array<[string, string]>
}

type PacketTemplate = Omit<Packet, "id" | "time">

export const serverIp = "192.168.10.157"
export const clientIp = "192.168.10.180"
export const gameKey = "00112233445566778899aabbccddeeff"
export const piaKey = "aabbccddeeff00112233445566778899"

export const packetTemplates: PacketTemplate[] = [
  {
    flow: "S → C", source: serverIp, destination: clientIp,
    protocol: "RTTProtocol", kind: "Type 0 Request", status: "authenticated",
    rawHex: "00 00 00 00 00 00 0b 71 c4 00 00 00 00 00 01 4c 20 00 00 00 00",
    fields: [["Type", "0"], ["OriginTick", "749764"], ["ElapsedMicroseconds", "85024"]],
  },
  {
    flow: "C → S", source: clientIp, destination: serverIp,
    protocol: "RTTProtocol", kind: "Type 1 Response", status: "authenticated",
    rawHex: "01 00 00 00 00 00 0b 71 c4 00 00 00 00 00 01 4c 20 00 00 00 01",
    fields: [["Type", "1"], ["OriginTick", "749764"], ["CaptureResponseDelayMs", "42.600"]],
  },
  {
    flow: "S → C", source: serverIp, destination: clientIp,
    protocol: "BroadcastReliable", kind: "PrivateSetting", status: "authenticated",
    rawHex: "07 00 46 00 0d 00 0d 00 1a 00 00 00 38 f4 06 f7 35 85 16 b6 56 56 07 07 43 6f 70 5f 53 68 61 6b 65 73 70 69 72 61 6c 00 00 00 00 00 00 00 00",
    fields: [["StageResource", "Cop_Shakespiral"], ["Difficulty", "1665"], ["SettingKind", "Common + Coop"]],
  },
  {
    flow: "S → C", source: serverIp, destination: clientIp,
    protocol: "UnreliableProtocol", kind: "CoopSequenceNetState", status: "authenticated",
    rawHex: "01 7e 2d 00 00 00 a2 08 03 12 44 00 00 00 00 00",
    fields: [["GroupId", "1104"], ["Sequence", "1"], ["Quota", "24"]],
  },
  {
    flow: "C → S", source: clientIp, destination: serverIp,
    protocol: "NetProtocol", kind: "Net 0x32", status: "authenticated",
    rawHex: "01 32 00 00 00 00 00 00 12 34 56 78 9a bc de f0",
    fields: [["Type", "0x32"], ["Role", "Client → Server"]],
  },
  {
    flow: "S → C", source: serverIp, destination: clientIp,
    protocol: "Browse", kind: "BrowseReply", status: "proof_verified",
    rawHex: "01 00 00 02 6c 0a 1d 2c 44 15 00 00 00 00 00 00 00 01 20 00 00 00 00 00",
    fields: [["HostIPv4", serverIp], ["PiaPort", "49152"], ["Proof", "verified"]],
  },
  {
    flow: "C → S", source: clientIp, destination: serverIp,
    protocol: "Pia", kind: "Pia frame", status: "authentication_failed",
    rawHex: "32 ab 98 64 8f 00 00 00 00 00 00 00 01 02 03 04 05 06 07 08 09 0a 0b 0c",
    fields: [["Error", "Authentication failed"]],
  },
]

export function demoTime(date = new Date()): string {
  return new Intl.DateTimeFormat("en-GB", {
    timeZone: "Asia/Tokyo",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  }).format(date)
}

export function makePacket(id: number, template: PacketTemplate): Packet {
  return { ...template, id, time: demoTime() }
}

export function initialPackets(): Packet[] {
  return Array.from({ length: 8 }, (_, index) =>
    makePacket(480 + index, packetTemplates[index % packetTemplates.length]!)
  ).reverse()
}

export function hexDump(hex: string): string {
  const bytes = hex.trim().split(/\s+/).map((value) => Number.parseInt(value, 16))
  const lines: string[] = []
  for (let offset = 0; offset < bytes.length; offset += 16) {
    const row = bytes.slice(offset, offset + 16)
    const values = row.map((value) => value.toString(16).padStart(2, "0")).join(" ")
    const ascii = row.map((value) => value >= 32 && value <= 126 ? String.fromCharCode(value) : ".").join("")
    lines.push(offset.toString(16).padStart(4, "0") + "  " + values.padEnd(47, " ") + "  |" + ascii + "|")
  }
  return lines.join("\n")
}
