# macOS Host 签名与钥匙串

签名标识为 `dev.devdesk.host` 的 Host 直接用 macOS Security Framework 操作钥匙串。发布包只包含这个可执行文件。服务名和账户名沿用此前的格式，数据库中的凭据引用继续指向原有条目。

## 源码构建

首次开发构建可准备并导入本机 DevDesk 签名身份。

```bash
./scripts/platform/macos/setup-macos-signing.sh --prepare
./scripts/platform/macos/setup-macos-signing.sh --import
./scripts/dev/build.sh
```

已有证书可用 `DEVDESK_CODESIGN_IDENTITY` 指定。保持同一个签名身份，macOS 才能按代码签名要求识别后续构建。发布包会核对签名及 Host 标识。开发期间可以用 `scripts/dev/run-host.py` 运行稳定的签名副本。直接运行 `target/release/devdesk-host` 时，重新构建前应结束旧进程。

## 为旧凭据授权

升级后如果钥匙串仍信任旧助手，首次访问该条目时需要用户确认新的签名 Host。模型的 API Key 可在服务商设置中点击钥匙串授权。后台读取遇到未授权条目时返回认证错误。

使用以下命令为所选数据集的 GitHub Token 授权。

```bash
./devdesk-host --data "/完整路径/DevDesk 数据" --authorize-credential github
```

如需授权其他集成，在同一命令中传入该集成配置保存的 `integration-secret:<UUID>` 引用。命令只读取现有条目以触发系统授权，也适用于设置中显示的模型凭据引用。授权完成后可以重试原来的操作。

## 验证

适配器的 [credentials_macos.rs](../../../crates/adapters/src/credentials_macos.rs) 检查签名和账户命名空间，并测试后台访问与显式授权互斥。实际钥匙串授权需使用签名后的 Host，在测试数据集上进行。
