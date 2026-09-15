# LAN control investigation findings

Investigation date: 2026-06-16 UTC log window, account `3009043526`.

## Root cause

- The TUI does prime LAN credentials for both `一楼入口` (`2045038922.s2`) and `一楼入口开关` (`2045038922`); subdevice requests are correctly aliased to root DID `2045038922`.
- The misleading path was after LAN discovery had no broadcast entry: the code tried legacy local UDP, but a transient UDP timeout (`Resource temporarily unavailable (os error 35)`) was treated like a permanent local credential failure.
- That permanent-failure path invalidated the ready credential. The current property chunk then returned `None`, `get_props_batch` logged `transport=cloud reason=fallback`, and following chunks often logged `missing-local-credential` before also going cloud.
- The same root-DID credential is shared by subdevices and main devices, so the bug affected both `2045038922.s2` and `2045038922`.

## Fix record

- `get_properties` now distinguishes LAN success, LAN unavailable, and transient LAN error.
- Transient local UDP errors retain the verified credential, record the request channel as `LAN`, and return per-property LAN error values instead of falling through to cloud.
- Property chunks are capped at 4 items to stay below the miIO UDP response-size ceiling.
- A transient chunk no longer performs a long single-property retry pass; it returns bounded LAN transient errors so the TUI refresh does not hang.
- The `not-discovered-online` log now says `next=local-udp`, because missing broadcast discovery is not itself cloud fallback.

## Reproduction after fix

- `cargo run -- tui`, device table, search `一楼入口`, open `一楼入口` (`2045038922.s2`), refresh props: log entries at `16:51:19Z`-`16:51:28Z` resolved every chunk as `LAN`; transient local UDP errors logged `credential=retained`; no new `transport=cloud` fallback appeared for `2045038922`.
- Open `一楼入口开关` (`2045038922`) and refresh props: log entries at `16:53:38Z`-`16:53:47Z` resolved every chunk as `LAN`; no cloud fallback appeared.

## Account LAN audit

- Hydrated from `~/.mit/accounts/3009043526/local_credentials.json`: 75 devices, 55 initially probe-ready.
- Audited non-air-condition devices: 75. Representative LAN read successes: 25. Failures: 50. Air conditioners skipped: 5.
- Audit method: one representative readable MIoT property per device, root-DID aliasing for `.sN` subdevices, serialized per LAN IP, one retry for transient private-LAN UDP timeout. Cloud reads were not counted as success.
- Raw audit JSON: `target/lan_audit.json`.

## Per-device results

| Status | Room | Device | DID | LAN IP | Property | Result / failure reason |
| --- | --- | --- | --- | --- | --- | --- |
| success | 一楼 | 一楼中枢 | 1195512009 | 192.168.0.82 | 网关 / 接入方式 | OK: representative property read over LAN |
| failed | 一楼 | 一楼入口 | 2045038922.s2 | 192.168.0.82 | 开关左键 / 左键 | no UDP reply from 192.168.0.82 after 2 attempts (os error 35) |
| failed | 一楼 | 一楼入口人在 | 1205551655 | 223.104.121.41 | 存在传感器 / 有人/无人状态 | Failed: snapshot localIp 223.104.121.41 is public/non-LAN; UDP timed out |
| success | 一楼 | 一楼入口开关 | 2045038922 | 192.168.0.82 | 开关左键 / 左键 | OK: representative property read over LAN |
| success | 一楼 | 一楼卧室人在 | 2033595723 | 192.168.0.37 | 传感器整体状态 / 有人无人状态 | OK: representative property read over LAN |
| failed | 一楼 | 一楼卧室灯 | 2000354988 | 192.168.0.82 | 开关 / 开关 | no UDP reply from 192.168.0.82 after 2 attempts (os error 35) |
| failed | 一楼 | 一楼卧室纱帘 | 2077342582 | 192.168.0.82 | 窗帘电机 / 工作状态 | no UDP reply from 192.168.0.82 after 2 attempts (os error 35) |
| success | 一楼 | 一楼卧室遮光 | 2023605158 | 192.168.0.82 | 窗帘电机 / 工作状态 | OK: representative property read over LAN |
| failed | 一楼 | 一楼厕所 | 2000352810 | 192.168.0.82 | 开关 / 开关 | no UDP reply from 192.168.0.82 after 2 attempts (os error 35) |
| success | 一楼 | 一楼厨房 | 2000494646 | 192.168.0.82 | 开关 / 开关 | OK: representative property read over LAN |
| failed | 一楼 | 一楼客厅主灯 | 2045025591 | 192.168.0.82 | 开关 / 开关 | no UDP reply from 192.168.0.82 after 2 attempts (os error 35) |
| failed | 一楼 | 一楼客厅人在 | 1205551784 | 223.104.121.41 | 存在传感器 / 有人/无人状态 | Failed: snapshot localIp 223.104.121.41 is public/non-LAN; UDP timed out |
| failed | 一楼 | 一楼客厅灯带 | 2045059050 | 192.168.0.82 | 开关 / 开关 | no UDP reply from 192.168.0.82 after 2 attempts (os error 35) |
| success | 一楼 | 一楼客厅窗帘 | 2023208757 | 192.168.0.82 | 窗帘电机 / 工作状态 | OK: representative property read over LAN |
| failed | 一楼 | 一楼楼梯上 | 2000446194.s3 | 192.168.0.82 | 开关左键 / 左键 | no UDP reply from 192.168.0.82 after 2 attempts (os error 35) |
| failed | 一楼 | 一楼楼梯下 | 2000446194.s2 | 192.168.0.82 | 开关左键 / 左键 | no UDP reply from 192.168.0.82 after 2 attempts (os error 35) |
| success | 一楼 | 一楼电梯口 | 2000446194 | 192.168.0.82 | 开关左键 / 左键 | OK: representative property read over LAN |
| failed | 一楼 | 一楼走廊 | 1161864579 | 192.168.0.82 | 雷达 / 有人无人状态 | no UDP reply from 192.168.0.82 after 2 attempts (os error 35) |
| success | 一楼 | 一楼院子 | 2045038922.s3 | 192.168.0.82 | 开关左键 / 左键 | OK: representative property read over LAN |
| failed | 一楼 | 中键-客厅 | 718342728.s15 | 192.168.0.82 | 显示屏 / 自动息屏 | no UDP reply from 192.168.0.82 after 2 attempts (os error 35) |
| failed | 一楼 | 右键-客厅 | 718342728.s16 | 192.168.0.82 | 显示屏 / 自动息屏 | no UDP reply from 192.168.0.82 after 2 attempts (os error 35) |
| success | 一楼 | 客厅 | 718342728 | 192.168.0.60 | 显示屏 / 自动息屏 | OK: representative property read over LAN |
| failed | 一楼 | 屏幕 | 718342728.s14 | 192.168.0.82 | 显示屏 / 自动息屏 | no UDP reply from 192.168.0.82 after 2 attempts (os error 35) |
| failed | 三楼 | 三楼中枢 | 1195422614 | 192.168.0.84 | 网关 / 接入方式 | no UDP reply from 192.168.0.84 after 2 attempts (os error 35) |
| failed | 三楼 | 三楼主卧 | 1205454448 | 223.104.121.41 | 存在传感器 / 有人/无人状态 | Failed: snapshot localIp 223.104.121.41 is public/non-LAN; UDP timed out |
| failed | 三楼 | 三楼主卧 | 2045081210 | 192.168.0.84 | 开关左键 / 左键 | Failed: 192.168.0.84 no route to host (os error 65) |
| failed | 三楼 | 三楼主卧窗帘 | 2023408874 | 192.168.0.84 | 窗帘电机 / 工作状态 | Failed: 192.168.0.84 host is down (os error 64) |
| failed | 三楼 | 三楼主卧门帘 | 2023408889 | 192.168.0.84 | 窗帘电机 / 工作状态 | Failed: 192.168.0.84 host is down (os error 64) |
| failed | 三楼 | 三楼主灯 | 2045081210.s2 | 192.168.0.84 | 开关左键 / 左键 | Failed: 192.168.0.84 host is down (os error 64) |
| failed | 三楼 | 三楼书房 | 2045025587 | 192.168.0.84 | 开关 / 开关 | Failed: 192.168.0.84 host is down (os error 64) |
| failed | 三楼 | 三楼书房纱帘 | 2023605175 | 192.168.0.84 | 窗帘电机 / 工作状态 | Failed: 192.168.0.84 host is down (os error 64) |
| failed | 三楼 | 三楼书房遮光 | 2077250328 | 192.168.0.84 | 窗帘电机 / 工作状态 | Failed: 192.168.0.84 host is down (os error 64) |
| failed | 三楼 | 三楼厕所 | 1205554788 | 223.104.121.41 | 存在传感器 / 有人/无人状态 | Failed: snapshot localIp 223.104.121.41 is public/non-LAN; UDP timed out |
| failed | 三楼 | 三楼厕所 | 2000495941.s3 | 192.168.0.84 | 开关左键 / 左键 | Failed: 192.168.0.84 host is down (os error 64) |
| failed | 三楼 | 三楼厕所门口 | 2000495941 | 192.168.0.84 | 开关左键 / 左键 | Failed: 192.168.0.84 host is down (os error 64) |
| failed | 三楼 | 三楼床头 | 2045054612 | 192.168.0.84 | 开关 / 开关 | Failed: 192.168.0.84 host is down (os error 64) |
| failed | 三楼 | 三楼座椅灯带 | 2000495941.s2 | 192.168.0.84 | 开关左键 / 左键 | Failed: 192.168.0.84 host is down (os error 64) |
| failed | 三楼 | 三楼楼梯上 | 2000499137.s3 | 192.168.0.84 | 开关左键 / 左键 | Failed: 192.168.0.84 host is down (os error 64) |
| failed | 三楼 | 三楼灯带 | 2045081210.s3 | 192.168.0.84 | 开关左键 / 左键 | Failed: 192.168.0.84 host is down (os error 64) |
| failed | 三楼 | 三楼电梯口 | 2000499137 | 192.168.0.84 | 开关左键 / 左键 | Failed: 192.168.0.84 host is down (os error 64) |
| failed | 三楼 | 三楼走廊 | 2000499137.s2 | 192.168.0.84 | 开关左键 / 左键 | Failed: 192.168.0.84 host is down (os error 64) |
| success | 二楼 | 二楼中枢 | 1195421309 | 192.168.0.83 | 网关 / 接入方式 | OK: representative property read over LAN |
| failed | 二楼 | 二楼主卧 | 2023408909 | 192.168.0.83 | 窗帘电机 / 工作状态 | no UDP reply from 192.168.0.83 after 2 attempts (os error 35) |
| success | 二楼 | 二楼主卧主灯 | 2000354982 | 192.168.0.83 | 开关 / 开关 | OK: representative property read over LAN |
| failed | 二楼 | 二楼书房 | 2000352806 | 192.168.0.83 | 开关 / 开关 | no UDP reply from 192.168.0.83 after 2 attempts (os error 35) |
| failed | 二楼 | 二楼书房纱帘 | 2077373923 | 192.168.0.83 | 窗帘电机 / 工作状态 | no UDP reply from 192.168.0.83 after 2 attempts (os error 35) |
| success | 二楼 | 二楼书房遮光 | 2023605200 | 192.168.0.83 | 窗帘电机 / 工作状态 | OK: representative property read over LAN |
| failed | 二楼 | 二楼储物排风扇 | 2000495921.s3 | 192.168.0.83 | 开关左键 / 左键 | no UDP reply from 192.168.0.83 after 2 attempts (os error 35) |
| failed | 二楼 | 二楼储物灯 | 2000495921.s2 | 192.168.0.83 | 开关左键 / 左键 | no UDP reply from 192.168.0.83 after 2 attempts (os error 35) |
| success | 二楼 | 二楼储物间 | 2000495921 | 192.168.0.83 | 开关左键 / 左键 | OK: representative property read over LAN |
| success | 二楼 | 二楼卧室 | 2033592177 | 192.168.0.38 | 传感器整体状态 / 有人无人状态 | OK: representative property read over LAN |
| failed | 二楼 | 二楼厕所 | 2000355003 | 192.168.0.83 | 开关 / 开关 | no UDP reply from 192.168.0.83 after 2 attempts (os error 35) |
| success | 二楼 | 二楼床头 | 2000406352 | 192.168.0.83 | 开关 / 开关 | OK: representative property read over LAN |
| failed | 二楼 | 二楼楼道上 | 2045063547.s3 | 192.168.0.83 | 开关左键 / 左键 | no UDP reply from 192.168.0.83 after 2 attempts (os error 35) |
| failed | 二楼 | 二楼洗手台 | 2000488048 | 192.168.0.83 | 开关 / 开关 | no UDP reply from 192.168.0.83 after 2 attempts (os error 35) |
| success | 二楼 | 二楼电梯口 | 2045063547 | 192.168.0.83 | 开关左键 / 左键 | OK: representative property read over LAN |
| failed | 二楼 | 二楼走廊 | 2045063547.s2 | 192.168.0.83 | 开关左键 / 左键 | no UDP reply from 192.168.0.83 after 2 attempts (os error 35) |
| success | 二楼 | 二楼走廊人存 | 1161875635 | 192.168.0.83 | 雷达 / 有人无人状态 | OK: representative property read over LAN |
| failed | 二楼 | 二楼阳台 | 2000354985 | 192.168.0.83 | 开关 / 开关 | no UDP reply from 192.168.0.83 after 2 attempts (os error 35) |
| success | 负一 | 中枢网关负一 | 1195517229 | 192.168.0.81 | 网关 / 接入方式 | OK: representative property read over LAN |
| failed | 负一 | 天井灯带 | 2045005249.s2 | 192.168.0.81 | 开关左键 / 左键 | no UDP reply from 192.168.0.81 after 2 attempts (os error 35) |
| success | 负一 | 影音室门口 | 2000352813 | 192.168.0.81 | 开关 / 开关 | OK: representative property read over LAN |
| failed | 负一 | 负一下楼楼梯灯 | 2000352812 | 192.168.0.81 | 开关 / 开关 | no UDP reply from 192.168.0.81 after 2 attempts (os error 35) |
| success | 负一 | 负一主灯 | 2045005249.s3 | 192.168.0.81 | 开关左键 / 左键 | OK: representative property read over LAN |
| failed | 负一 | 负一吸顶灯 | 1138501848 | 192.168.0.81 | 灯 / 开关 | no UDP reply from 192.168.0.81 after 2 attempts (os error 35) |
| success | 负一 | 负一夹层灯 | 2045025580 | 192.168.0.81 | 开关 / 开关 | OK: representative property read over LAN |
| failed | 负一 | 负一电梯口 | 2045005249 | 192.168.0.81 | 开关左键 / 左键 | no UDP reply from 192.168.0.81 after 2 attempts (os error 35) |
| success | 负二 | 中枢负二 | 1195422632 | 192.168.0.80 | 网关 / 接入方式 | OK: representative property read over LAN |
| success | 负二 | 负二人在 | 2033596721 | 192.168.0.26 | 传感器整体状态 / 有人无人状态 | OK: representative property read over LAN |
| failed | 负二 | 负二储藏室 | 2000346189 | 192.168.0.80 | 开关 / 开关 | no UDP reply from 192.168.0.80 after 2 attempts (os error 35) |
| success | 负二 | 负二大厅灯 | 2000352829 | 192.168.0.80 | 开关 / 开关 | OK: representative property read over LAN |
| failed | 负二 | 负二楼梯灯带 | 2000354989 | 192.168.0.80 | 开关 / 开关 | no UDP reply from 192.168.0.80 after 2 attempts (os error 35) |
| success | 负二 | 负二灯带 | 2045029978.s2 | 192.168.0.80 | 开关左键 / 左键 | OK: representative property read over LAN |
| failed | 负二 | 负二电梯口开关 | 2045029978 | 192.168.0.80 | 开关左键 / 左键 | no UDP reply from 192.168.0.80 after 2 attempts (os error 35) |
| failed | 负二 | 负二电梯灯 | 2045029978.s3 | 192.168.0.80 | 开关左键 / 左键 | no UDP reply from 192.168.0.80 after 2 attempts (os error 35) |

## Skipped air conditioners

| Room | Device | DID | Model |
| --- | --- | --- | --- |
| 二楼 | 二楼书房 | x.2069.3009043526.86100c00900202300000fffec80a358408e1 | juhl.aircondition.a11 |
| 一楼 | 一楼客厅 | x.2069.3009043526.86100c009005003000000040021d61f4 | juhl.aircondition.a11 |
| 二楼 | 二楼主卧 | x.2069.3009043526.86100c00900202300000fffec80a3583e58d | juhl.aircondition.a11 |
| 三楼 | 三楼主卧 | x.2069.3009043526.86100c009005003000000040da4570ce | juhl.aircondition.a11 |
| 一楼 | 一楼主卧 | x.2069.3009043526.86100c00900202300000fffec80a3584b269 | juhl.aircondition.a11 |
