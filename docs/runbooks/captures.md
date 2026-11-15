# Capture runbook (maintainer-run)

Captures are the only evidence diagwire accepts for protocol claims (DR-0001 D3/D4). Development agents never start hardware I/O (D7), so every command that talks to a car here is run by the maintainer. An agent may prepare config and read the resulting files.

Nothing captured goes into the repo until it has a provenance stamp and a redaction review (D4/D8). Originals stay private, outside the repo, referenced by hash.

## 1. M3 K-line trace check (epic #1, slice 1)

**Question it settles:** does EDIABAS's interface trace show raw DS2 telegrams, including the K-line echo? If yes, EDIABAS is the DS2 capture tool. If no, a passive serial tap is needed.

1. Back up `C:\EDIABAS\Bin\EDIABAS.INI`.
2. In that file set `IfhTrace = 3` (currently `0`; the file documents 2 as "user trace + interface" and 3 as adding a time stamp). Leave `ApiTrace = 0`. `TracePath` is `C:\EDIABAS\TRACE`.
3. K+DCAN cable on the 20-pin connector, ignition in position 2, no other BMW tool open (COM1 lock).
4. Maintainer runs one fault read:
   ```
   "/c/Windows/SysWOW64/WindowsPowerShell/v1.0/powershell.exe" -NoProfile -ExecutionPolicy Bypass -File "C:\Users\User\Projects\m3\ediabas.ps1" -Ecu ms411ds1 -Job FS_LESEN
   ```
5. Set `IfhTrace = 0` again (or restore the backup).
6. An agent reads the newest files in `C:\EDIABAS\TRACE` and records the finding in `docs/notes/`: raw bytes visible or not, echo visible or not, timestamps or not.

Stop condition: the trace has no raw bytes. Record that, and decide on a serial tap before any DS2 transport work.

## 2. X5 HSFZ capture (epic #1, slice 2)

**Needs:** the ENET cable, a canonical client that can talk to a G05, and a packet capture on the laptop's Ethernet port.

**Open risk (unresolved):** no canonical client on this laptop is known to reach a G05. The installed ISTA build is from 2016 and predates the G05, E-Sys is not installed, and it is unverified whether the installed EDIABAS data has an SGBD for the G05's DME. DR-0001's stop condition applies: if no canonical client reaches the X5, the slice is parked and the reason recorded. A capture with diagwire itself as the tester is characterization only and can still read the car's faults, but it cannot back a conformance claim.

Steps once a client is chosen:
1. Cable in, ignition on. Start a packet capture on the Ethernet adapter. Windows' built-in `pktmon` (admin) is one option, unverified on this machine:
   `pktmon start --capture --pkt-size 0 -f x5.etl`, then `pktmon stop` and `pktmon etl2pcap x5.etl`.
2. With the canonical client: discover the car, read the DME's fault memory, then idle about a minute so alive checks show up. Save the client's decoded fault list.
3. Stop the capture. Keep the original outside the repo and note its SHA-256.
4. Stamp (car, client and version, capture tool, date, host), list the transformations, and review redaction (VIN, MAC and IP addresses) before anything is committed.

The capture also settles the open port question in DR-0001: whether discovery is UDP to 6811 (the spike) or 6811 is a TCP control channel (as `EDIABAS.INI` describes it).
