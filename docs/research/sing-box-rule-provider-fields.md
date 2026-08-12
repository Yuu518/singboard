# sing-box 规则提供者字段语义

## 结论

sing-box 的规则集配置把 `source` / `binary` 定义为 `format`（规则集格式），不是规则“行为”。官方文档明确将 `format` 描述为规则集文件格式，并限定取值为 `source` 或 `binary`；官方源码也将该字段声明为 `enum:"source,binary"`。[官方文档](https://sing-box.sagernet.org/configuration/rule-set/#format) · [官方源码](https://github.com/SagerNet/sing-box/blob/c434966a03e85e84da5be6097f33066db309caee/option/rule_set.go#L22-L25)

当前受支持的扩展实现为兼容 Clash API 的既有响应结构，把 `ruleSet.Format()` 写进了 JSON 的 `behavior` 属性，并转为大写。因此接口中看到的 `behavior: "SOURCE"` / `"BINARY"` 在语义上仍是 sing-box 的**格式**；它只是借用了兼容 API 的 `behavior` 键。该实现没有输出单独的 `format` 属性。[兼容 API 映射源码](https://github.com/Yuu518/sing-box/blob/b8a64ba4081359459af5c4e6de814c9f994a1041/experimental/clashapi/ruleprovider.go#L28-L35)

## 字段映射

| 响应属性 | 受支持的 sing-box 实现实际写入 | 可能值 | 准确含义 | 建议 UI |
| --- | --- | --- | --- | --- |
| `behavior` | `strings.ToUpper(ruleSet.Format())` | `SOURCE`, `BINARY` | 规则集格式 | 列名“格式”；值保留接口英文原值 |
| `format` | 未输出 | — | sing-box 配置中的正式字段名，但不是该兼容响应中的属性 | 不要把它设为读取该接口的必填字段 |
| `type` | 常量 `Rule` | `Rule` | 提供者类别，即规则提供者 | 信息重复，可不展示；若展示为“规则” |
| `vehicleType` | `strings.ToUpper(ruleSet.Type())` | `INLINE`, `LOCAL`, `REMOTE` | 规则集来源/装载类型 | 列名“来源”（保留“载入方式”也可）；值显示“内联”“本地”“远程” |

sing-box 官方配置也把 `type` 限定为 `inline`、`local`、`remote`，并根据 `.json` / `.srs` 后缀分别推导 `source` / `binary`。[类型和格式声明](https://github.com/SagerNet/sing-box/blob/c434966a03e85e84da5be6097f33066db309caee/option/rule_set.go#L22-L25) · [后缀推导](https://github.com/SagerNet/sing-box/blob/c434966a03e85e84da5be6097f33066db309caee/option/rule_set.go#L124-L133)

## 为什么属性名会显得不一致

在原生 Mihomo/Clash 规则提供者响应中，`behavior` 通常表示匹配策略（`Domain`、`IPCIDR`、`Classical`），`format` 表示文件格式（`YamlRule`、`TextRule`、`MrsRule`），`vehicleType` 表示读取载体，`type` 表示提供者类别。[Mihomo API 序列化](https://github.com/MetaCubeX/mihomo/blob/e26714a181ac0e2fa803453c0a8e9a9ce94e31cb/rules/provider/provider.go) · [Mihomo 枚举定义](https://github.com/MetaCubeX/mihomo/blob/e26714a181ac0e2fa803453c0a8e9a9ce94e31cb/constant/provider/interface.go)

sing-box 规则集没有与 Clash 的 `domain` / `ipcidr` / `classical` 一一对应的单一行为值，所以该扩展实现复用了兼容响应的 `behavior` 槽来传递 sing-box 的格式。前端应按此实现的真实语义命名列，但保留读取 JSON 键 `behavior`。

## 版本边界

上述响应映射以提交 `b8a64ba4081359459af5c4e6de814c9f994a1041`（2026-08-10）为准。相同时点的 SagerNet 官方上游仍让 `/providers/rules` 返回空列表，并未定义这些详情字段；因此不要把这个映射表述成所有官方 sing-box 版本都保证的公共 API。[官方上游端点实现](https://github.com/SagerNet/sing-box/blob/c434966a03e85e84da5be6097f33066db309caee/experimental/clashapi/ruleprovider.go#L22-L26)

## 最小界面改动建议

- 将绑定 `provider.behavior` 的列标题由“行为”改为“格式”。
- 保留 `SOURCE` / `BINARY` 英文原值，便于与 sing-box 文档和配置直接对应。
- 可将 `vehicleType` 的“载入方式”改为更直接的“来源”，并映射 `INLINE` / `LOCAL` / `REMOTE` → “内联” / “本地” / “远程”。
- 不需要重命名接口模型中的 `behavior` 属性；它是当前线协议键，只需在展示层纠正术语。
