# 界面语言

应用只使用简体中文。词典代码是 `zh`，HTML 语言和数字、日期格式是 `zh-CN`，地区是 `CN`。

资源在 `src/i18n/resources.ts` 静态导入，`create-i18n.ts` 用同一份配置创建 i18next 实例。应用、Vitest 和 Playwright 各自创建实例；Playwright 不导入 `i18n.ts`。

初始化使用 `initAsync: false`，在 React 首次渲染前完成。缺键不回退到其他语言。外部名称可以在调用处保留 `defaultValue`。

检查：

```bash
pnpm --filter frontend i18n:check
node --test apps/frontend/scripts/check-translations.test.mjs
```
