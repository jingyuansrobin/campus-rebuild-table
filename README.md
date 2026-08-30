# MCRebuild V3

MCRebuild V3 是面向高校 Minecraft 社团的校园数字复刻工作台。

核心目标：让一个没有 GIS 专业背景的高校 Minecraft 社团，能够快速从真实校园获得一个可继续施工的约 60 分基础校园，再由社团成员在 Minecraft 中精修到约 90 分。

## V3 产品边界

V3 当前采用：

- Native Desktop 作为主要用户入口；
- Headless Core + CLI 作为可测试、可复用的核心能力；
- Arnis 作为 V3.0 唯一基础生成引擎；
- Local-first Campus Project；
- Cloud-first Asset Hub（分阶段实现）；
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

## 仓库定位

本仓库是 MCRebuild V3 的主开发仓库。

旧仓库 `jingyuansrobin/campus-reconstruction-tool` 作为 V2 产品基线、历史实现和成熟代码参考，不做整仓复制；V3 只选择性迁移仍服务于新产品闭环的能力。

## 初始工程结构

```text
apps/
  cli/                 # Headless 开发与验证入口
  desktop/             # 后续接入的薄桌面壳
crates/
  campus-core/         # CampusProject / RealityModel / CampusObject 等领域模型
  app-core/            # 应用用例
  arnis-adapter/       # Arnis 适配边界
docs/
  architecture/        # V3 架构
  migration/           # V2 → V3 迁移决策
  implementation/      # Vertical slice 与实施计划
```

## 开发原则

1. 先跑通 vertical slice，再扩模块。
2. 不因为未来可能需要而提前建设复杂抽象。
3. UI 保持 Thin Shell，业务规则进入 headless core。
4. Arnis 不渗透进 MCRebuild 领域模型，通过适配层隔离。
5. 项目数据优先可读、可迁移、可 diff。
6. V3.0 不重新实现 Arnis 已经解决的基础 GIS → Minecraft 生成能力。
