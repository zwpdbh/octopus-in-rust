# FAFCN 维护备忘（2026-08-28 更新）

> 密钥类只记录**存放位置**，不记录明文。明文只存在于下列指明的文件/控制台中。

## 一、网站基本信息

| 项目 | 值 |
|---|---|
| 公网地址 | **https://faforever.cn:60**（旧地址 `https://8v.pub:10041` 为遗留别名，勿依赖） |
| 域名 | `faforever.cn`，**阿里云注册**（个人实名），注册 2026-08-25，**到期 2027-08-25** |
| ⚠️ 续期提醒 | **2027 年 7 月前在阿里云控制台续费**，过期 = 全站下线 |
| 为什么带端口 | 朋友家宽 ISP 封锁 80/443，只能用非标端口（当前 `:60`） |
| ICP 备案 | 未备案；.cn + 大陆 IP 理论上可能被要求备案，当前非标端口运行正常。若被要求，回退方案：回 `8v.pub` 或迁移到有备案的主机 |

## 二、架构与职责划分

```
玩家浏览器 / fafcn-sync
        │ https://faforever.cn:60
        ▼
朋友的 Lucky 网关（113.5.92.224，动态 IP）
  · DDNS：自动同步 A 记录 → 阿里云 DNS
  · 证书：Let's Encrypt，ACME DNS-01，自动续期
  · 反代：faforever.cn:60 → 192.168.50.10:3000
  · 端口映射：10040→22（SSH）、60→3000（网站）
        ▼  plain HTTP
majiko 家用服务器 192.168.50.10（Ubuntu 22.04）
  · fafcn-server 单端口 3000 承载一切（web/API/WebSocket/gamedata 上下行）
  · systemd 单元：fafcn.service，安装目录 /opt/fafcn（属 majiko）
```

| 谁管什么 | 内容 |
|---|---|
| **我（阿里云 CLI 可直接操作）** | 域名续费、DNS 记录（`aliyun` CLI profile `default`）、代码与部署 |
| **朋友（Lucky 网关）** | DDNS 任务、证书签发续期、反代规则、端口映射 |
| **朋友的 DNS Key** | 阿里云 **RAM 子账号**，权限锁死在 `faforever.cn` DNS（策略 `acs:alidns:*:*:domain/faforever.cn`）。泄漏则在 RAM 控制台吊销重建。**绝不外发主账号或本机 CLI 的 AK** |

## 三、部署与运维命令（本机仓库根目录）

```bash
cargo xtask fafcn majiko-deploy              # 全量部署（后端+wasm插件+前端）
cargo xtask fafcn majiko-deploy --skip-web   # 只更新后端
cargo xtask fafcn majiko-deploy-file-sync    # 只更新 fafcn-sync Windows 客户端
cargo xtask fafcn majiko-health              # 三层体检（SSH→服务→公网），排障第一步
```

- 部署配置：`xtask/.env`（git-ignored）——`MAJIKO_SSH_PASSWORD`（SSH/sudo 密码明文在此）、`MAJIKO_PUBLIC_URL=https://faforever.cn:60`
- SSH：`ssh -p 10040 majiko@8v.pub`（密码同上；sudo 同密码）
- 服务器日志：`journalctl -u fafcn -f` 或 `/opt/fafcn/data/logs/fafcn-server.log`

## 四、密钥清单（位置索引）

| 密钥 | 存放位置 |
|---|---|
| majiko SSH/sudo 密码 | 本机 `xtask/.env` |
| LLM API key（secsino 转发站，朋友账号） | 服务器 `/opt/fafcn/.env`（`FAFCN_LLM_*`） |
| gamedata 上传 token（UPLOAD_TOKEN） | 服务器 `/opt/fafcn/.env` |
| FAF OAuth client_id/secret（**待收到**） | 收到后写服务器 `/opt/fafcn/.env` + 本机 `apps/fafcn-server/.env` |
| 旧 Kimi LLM 配置备份 | 服务器 `/opt/fafcn/.env.bak-kimi`（可回滚） |
| 阿里云 CLI AK（本机） | `aliyun configure list`（profile `default`） |

⚠️ 服务器 `.env` 与本机 `apps/fafcn-server/.env` **不一致**（LLM 已换转发站），不要直接覆盖；Q&A 是公开功能，**每个访客提问都在烧朋友 key 的额度**。

## 五、fafcn-sync 客户端要点

- 玩家配置：`%APPDATA%\fafcn-sync\config.toml`（server 地址等）
- exe 尾部内嵌下载来源地址（服务器按请求 origin 写入），仅作首次运行兜底；用户保存的地址优先（2026-08-28 三连修：启动覆盖 / 关窗不存 / 自更新硬退不存）
- 自更新：`std::process::exit` 硬切换 exe；新 build 通过 `majiko-deploy-file-sync` 发布，玩家在客户端点「检查更新」即可
- 当前最新 build：`dev-6a90fdaf-6052`

## 六、FAF 官方对接状态

- OAuth 申请**已批准**（Brutus5000，consent 名 `fafcn`），**凭据未收到**
- 已通知对方更新 prod Redirect URI：`https://faforever.cn:60/api/auth/callback`（含端口精确白名单）
- 拿到凭据后按 `docs/fafcn/faf-integration.md` §2.1 checklist 执行

## 七、排障决策树

1. `cargo xtask fafcn majiko-health` —— 看哪层红
2. 服务层红 → 上服务器 `journalctl -u fafcn -n 50`
3. 公网层红、服务层绿 → 朋友那边：边缘转发断了（内网 IP 变了）/ DDNS 没同步（公网 IP 变了）/ 证书问题
4. 公网端口出现 `CN=Lucky` 自签证书 → 朋友的 Lucky 上域名 vhost/证书绑定坏了
5. `faforever.cn` 解析不到或 IP 不对 → 朋友的 DDNS 任务挂了（或域名忘续费）

## 八、关键文档

- 部署 runbook：`docs/fafcn/how_to_deploy_fafcn_on_majiko.md`（本备忘的详版）
- FAF 对接设计：`docs/fafcn/faf-integration.md`
- 通用部署（新服务器）：`docs/fafcn/how_to_deploy_fafcn.md`
