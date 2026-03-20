# Monitor Cleaner

一键清空/恢复显示器上所有窗口的 Windows 小工具。

## 功能

- `Ctrl + Q` 切换当前鼠标所在显示器的窗口状态
- 第一次按下：最小化该显示器上的所有窗口
- 再次按下：恢复之前最小化的窗口（保持原有顺序）
- 后台运行，系统托盘图标，右键退出

## 构建

```bash
cargo build --release
```

生成的 exe 在 `target/release/toggle_monitor_cleaner.exe`

## 自定义图标

替换 `resources/icon.ico` 为你喜欢的图标，重新构建即可。

## 依赖

- Windows 10+
- Rust 1.70+

## License

MIT
