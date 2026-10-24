# 来源和主要修改

Wealthlog 以 Wealthfolio 的
[`bfcd6d6202ceb4922d0be85aa5666d1802eda456`](https://github.com/wealthfolio/wealthfolio/tree/bfcd6d6202ceb4922d0be85aa5666d1802eda456)
为基准，于 2026-10-13 开始修改，由 Wealthlog 贡献者独立维护。
整体按 AGPL-3.0 发布，原有源码版权声明保留。

主要修改：

- 将产品名称、应用标识、图标和发布配置改为 Wealthlog；保留数据库与密钥兼容所需的历史内部标识。
- 裁剪云同步、自动更新、内置聊天、Addon、目标工具、原生移动入口及导入和用户恢复入口。
- 增加普通收支 CLI、桌面服务生命周期、Agent 接入和本地技能安装。
- 移除上游证券及行情提供商 Logo 素材，保留用户自行添加的资产图片与文字头像。旧资料中的头像 ID 保持有效，以默认图标显示，不修改账本数据。

Wealthfolio is a trademark of Teymz Inc. 本项目与上游没有隶属关系，也未获得其背书。
