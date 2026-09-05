# MCRebuild V3

MCRebuild V3 是面向高校 Minecraft 社团的校园数字复刻工作台。

核心目标：让一个没有 GIS 专业背景的高校 Minecraft 社团，能够快速从真实校园获得一个可继续施工的约 60 分基础校园，再由社团成员在 Minecraft 中精修到约 90 分。

## V3 产品边界

V3 当前采用：

- Native Desktop 作为主要用户入口；
- Headless Core + CLI 作为可测试、可复用的核心能力；
- Arnis 作为 V3.0 唯一基础生成引擎；
- Local-first Campus Project；
- Cloud-first Asset Hub 作为长期共享资产库，已下载资产保留本地缓存并分阶段实现 Hub 能力；
- Minecraft 精细施工保持单向输出，V3.0 不做回流同步；
- 当前不涉及 AI / Agent。

## V3.0 首个产品闭环

```text
搜索学校
→ 选择校区并自动创建 CampusProject
→ 确认 / 调整多边形校园边界
→ 设置生成尺度（1.0–2.5 block/m）与可选朝向
→ 调用 Arnis
→ 预览 60 分基础校园
→ 打开生成结果进入 Minecraft 精修
```

## 当前进度

### v0.1 — Local CampusProject ✅

Headless Core 可以创建本地项目目录并持久化 `project.json`、`reality.json` 与 `objects.json`。

### v0.2 — 高校 / 校区搜索 ✅

V3 不复制 V2 的 WebView 搜索实现。搜索通过独立 Rust 适配器调用高德 Web Service，地图 WebView 只负责后续边界展示和编辑。

```text
CLI / future Desktop
        ↓
gaode-search
        ↓
AMap Web Service（仅高等院校 141201）
        ↓
CampusCandidate
        ↓
app-core
        ↓
CampusProject + RealityModel
```

高德 Web Service key 通过运行时环境变量提供：

```text
AMAP_WEB_SERVICE_KEY
```

开发阶段 CLI：

```text
mcrebuild-cli search-campus "华东师范大学" "上海市"
mcrebuild-cli init-campus ./ecnu "华东师范大学" <poi_id> 1.5 "上海市"
```

### v0.3a — WGS-84 权威多边形边界 ✅

项目边界已经从 V2 的弱类型 GeoJSON JSON 包装重写为 `CampusBoundary`：

- Core 内部统一使用 WGS-84；
- 高德 GCJ-02 坐标在适配器边界转换；
- 至少三个有效顶点；
- 拒绝连续重复点、自相交和过小多边形；
- `project.json` 保存权威多边形；
- Arnis 所需矩形 BBOX 只能由多边形派生，不能反过来替代项目边界。

开发阶段可以用 CLI 验证边界：

```text
mcrebuild-cli set-boundary ./ecnu "121.398,31.222;121.414,31.222;121.414,31.234;121.398,31.234"
```

Arnis 当前仍以矩形 BBOX 为主要生成入口。因此在 polygon mask / crop 真正落地以前，MCRebuild 不宣称已经实现“任意形状生成结果裁切”。

### v0.3b — 地图边界编辑器 ✅ code path / ⚠ runtime smoke test pending

V3 不迁移 V2 约 57 KB 的大体量边界 WebView 页面，也不恢复 Slint 内嵌 Wry 子窗口架构。

当前路线：

```text
Windows Native Window
        ↓
Wry WebView（薄壳）
        ↓
gaode-map（HTML / GCJ-02↔WGS-84 / IPC）
        ↓
app-core
        ↓
CampusBoundary 校验 + project.json 原子保存
```

地图 IPC 目前只有三个事件：`ready`、`cancel`、`submit_boundary`。地图页负责画和拖，Core 负责最终校验与持久化。

当前 Windows 开发入口：

```powershell
$env:AMAP_JS_KEY="<your-js-api-key>"
$env:AMAP_JS_SECURITY_CODE="<your-security-code>"
cargo run -p mcrebuild-desktop -- .\ecnu
```

该入口直接打开一个已经由高校搜索创建、且拥有地理锚点的 CampusProject。它是 vertical slice 验证入口，不是最终产品首页。

CI 已覆盖 Windows Desktop 编译，但真实 Windows + WebView2 + 高德 JS key/origin 仍需要 smoke test；编译成功不等于地图运行时已经用户验证。

不要把任何高德 key 或 security code 写入源码、CampusProject 或 Git。

### v0.4 — Arnis 生成闭环 ✅

V3 不重新实现 GIS → Minecraft 基础生成，而是通过 `arnis-adapter` 调用 Arnis 外部进程。

当前 headless 生成链：

```text
CampusProject
  ├─ generation_target = minecraft_java
  ├─ generation_scale = 1.0–2.5 block/m
  └─ CampusBoundary (WGS-84 polygon)
          ↓
派生 transport BBOX
          ↓
arnis-adapter
          ↓
arnis --output-dir <staging-parent>
      --bbox <minLat,minLng,maxLat,maxLng>
      --scale <blocks/m>
      --mode geo-terrain
          ↓
Arnis 在 staging parent 下创建 Java world 子目录
          ↓
写入 .mcrebuild-generation.json
          ↓
原子提升为 generated/world
```

开发阶段生成命令：

```text
mcrebuild-cli generate ./ecnu
```

默认从 `PATH` 查找 `arnis`。也可以通过运行时环境变量指定：

```text
ARNIS_EXECUTABLE=<path-to-arnis>
```

项目现在只声明产品级目标 `Minecraft Java`，不再保存一个 Arnis 无法保证兑现的任意 `minecraft_version`。具体世界格式由实际使用的 Arnis 二进制决定，并写入生成产物 metadata；旧项目 JSON 中的 `minecraft_version` 仍可兼容读取，但不控制 Arnis。

当前最重要的限制：MCRebuild 保存的是任意多边形边界，但 Arnis 当前 CLI 接收的是矩形 BBOX。v0.4 会将多边形的 bounding box 作为 transport boundary，并在 `.mcrebuild-generation.json` 中记录 `boundary_transport = polygon_bounding_box`。这意味着 bbox 四角落在校园多边形之外的区域目前仍可能被生成，不能把 v0.4 宣称为任意形状裁切。

生成采用临时目录事务：失败产物留在正式 `generated/world` 之外；成功且识别到唯一包含 `level.dat` 的 Java world 后才发布。已有 `generated/world` 时默认拒绝覆盖。

除 fake-process 集成测试外，v0.4 已用固定的 Arnis v3.1.0 Linux release 做真实网络 smoke test：MCRebuild CLI 成功驱动 Arnis 获取地图 / 地形数据并发布 Minecraft Java world，生成 manifest 中的 `generator_version` 严格记录为 `3.1.0`。Live smoke 与常规 PR CI 分离，避免公共数据源或上游网络波动阻塞日常开发。

### v0.5a — 可观察、可取消的生成生命周期 ✅

交互式客户端不再只能阻塞等待 Arnis 结束。`app-core` 现在提供 provider-neutral 的生成生命周期接口：

```text
Desktop / CLI / future agent
        ↓
GenerationEvent
  ├─ Stage(PreparingData | ProcessingMap | GeneratingWorld | SavingWorld)
  └─ Log(stdout | stderr, raw line)
        ↓
app-core
        ↓
arnis-adapter
        ↓
Arnis child process
```

同时提供 clone-shared 的 `GenerationCancellationToken`。取消时 adapter 终止直接 Arnis 子进程，app-core 清理该次 staging root，并且不会发布不完整的 `generated/world`。CI 中有实际启动长时间运行 fake-Arnis 后再取消的集成测试，不只是测试一个布尔 flag。

这里刻意**不提供 0–100% 数字进度**。Arnis CLI 当前只输出面向人的阶段文本，真实运行中 `[3/7]` 可能先于 `[2/7]` 出现；把这些数字直接包装成进度百分比会制造虚假精度。v0.5a 只暴露单调推进的四个粗粒度阶段和原始日志。

当前取消保证针对直接 Arnis 子进程。只有在真实运行证明 Arnis 会留下需要处理的子孙进程时，才增加 Windows Job Object / Unix process group 等进程树管理；V3.0 不为假设提前建设这层复杂度。

### v0.5b — Desktop 生成 Worker ✅ code path / ⚠ Windows runtime smoke test pending

Windows Desktop 已把 v0.5a 生命周期接入 winit/Wry 事件循环：

```text
WebView generation panel
        ↓ IPC: start_generation / cancel_generation
Windows Desktop shell
        ↓ spawn worker thread
app-core generate_project_with_arnis_observed
        ↓ GenerationEvent
EventLoopProxy<UserEvent>
        ↓
Wry UI: stage / raw logs / finished
```

生成不再阻塞 UI 线程。界面提供“开始生成”、四阶段状态、最多 300 行原始 stdout/stderr 日志和“取消”。生成期间 Core 项目边界禁止写入，避免用户一边生成一边修改权威边界导致一次生成过程内出现语义漂移。

用户在生成期间关闭窗口时，Desktop 不立即退出进程，而是先触发 cancellation，等待 app-core/adapter 完成 staging 清理并返回，再退出事件循环。`ARNIS_EXECUTABLE` 可指定 Arnis 二进制；未设置时仍使用 `PATH` 中的 `arnis`。

当前生成面板是 Desktop 在边界页 Ready 后注入的 **vertical-slice 过渡 UI**。它的好处是没有把生成职责塞进 `gaode-map`，但它不应演化为长期页面架构。更成熟的最终形态应是 Desktop/应用壳拥有页面布局，地图只是其中一个组件，而不是持续向地图页追加 DOM。

Windows CI 已真实编译该 worker / `EventLoopProxy` / Wry 路径，但还没有在真实 Windows + WebView2 + 高德 JS + Arnis 组合环境中完成 GUI smoke。因而当前只能宣称“代码路径和平台编译成立”，不能宣称 Desktop 生成体验已经用户验证。

## 仓库定位

本仓库是 MCRebuild V3 的主开发仓库。

旧仓库 `jingyuansrobin/campus-reconstruction-tool` 作为 V2 产品基线、踩坑记录和参考实现，不做整仓复制。每个旧能力在进入 V3 前都先按产品价值与实现质量判断：保留思路、选择性复用、重写或删除。

## 工程结构

```text
apps/
  cli/                 # Headless 开发与验证入口
  desktop/             # Windows Native + Wry 薄桌面壳
crates/
  campus-core/         # CampusProject / RealityModel / CampusBoundary / CampusObject
  app-core/            # 应用用例与项目文件访问边界
  gaode-search/        # 高德高校 / 校区搜索适配器
  gaode-map/           # 高德边界地图、CRS 转换与小型 IPC 协议
  arnis-adapter/       # Arnis CLI / 进程 / 输出目录适配边界
docs/
  architecture/        # V3 架构
  migration/           # V2 → V3 迁移决策
  implementation/      # Vertical slice 与实施记录
```

## 开发原则

1. 先跑通 vertical slice，再扩模块。
2. 不因为未来可能需要而提前建设复杂抽象。
3. UI 保持 Thin Shell，业务规则进入 headless core。
4. Arnis 不渗透进 MCRebuild 领域模型，通过适配层隔离。
5. 项目数据优先可读、可迁移、可 diff。
6. V3.0 不重新实现 Arnis 已经解决的基础 GIS → Minecraft 生成能力。
7. V2 只作为参考实现；迁移前先审计产品价值和实现质量，允许重写或删除。
8. 完整 vertical slice 通过 feature branch + CI + PR 合并到 `main`，避免中间失败状态直接进入主线。
9. 平台专属代码必须由对应平台 CI 编译；Windows Desktop 不以 Linux stub 通过作为质量证明。
10. 外部生成器的版本、CLI 参数和输出目录语义属于 adapter；Core 不依赖 Arnis 内部命名约定。
