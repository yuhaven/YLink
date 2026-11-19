# DoD 与交付清单

没有全部勾选，不得在交付说明里宣布该功能完成，也不得请用户「先试试」。

```markdown
- [ ] 需求卡已写，成功标准是可观察量（用户点哪里、几秒内看到什么）
- [ ] 有歧义的原话已对齐，或用户已从选项中选定
- [ ] 涉及界面：竞品拆解表已确认（含截图与对照结论）
- [ ] 平台能力已确认，未自研已有能力
- [ ] 首屏可交互元素 ≤ 5；若新增控件，已说明删掉哪个
- [ ] 每个控件都有「变更 → 应用到当前会话 → 验收」路径和用例
- [ ] 能实时生效的设置没有被做成「下次启动才生效」
- [ ] 必须重启的设置会提示「正在应用」，重启后会话状态正常
- [ ] L3 E2E 覆盖了用户真实点击路径并通过
- [ ] 测试前已隔离：旧进程 / settings.json / WebView2
- [ ] 发布只走 `npm run tauri -- build`，界面版本号正确
- [ ] 发布包默认配置已校验，测试产物未进入 zip
- [ ] 文档与实现同提交
- [ ] 已按首次使用 / 二次使用 / 异常路径自测 3 遍，并附截图、数值或日志
- [ ] 交付说明写明「能做什么 / 不能做什么 / 已知问题」
- [ ] 有回滚到上一版的路径
```

常用验收命令：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\e2e_cast.ps1 -Orientation landscape
powershell -ExecutionPolicy Bypass -File scripts\e2e_cast.ps1 -Orientation portrait
Get-Content "$env:TEMP\castpane.log" -Wait | Select-String "apply_settings|applied"
Get-Content dist\CastPane\settings.json
```
