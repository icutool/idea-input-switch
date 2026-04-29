# Url Sender

一个基于 Chrome Manifest V3 的浏览器插件。

功能说明：

- 点击插件图标后，可以配置后台地址。
- 可以分别配置“中文输入法网址正则”和“英文输入法网址正则”。
- 每行一个正则，按整条 URL 做匹配。
- 当访问新的网页地址时：
  - 命中中文规则，调用 `baseUrl/switch?mode=1`
  - 命中英文规则，调用 `baseUrl/switch?mode=0`
- 中文规则优先于英文规则。
- 命中后插件图标会短暂显示 `CN`、`EN` 或 `ERR`。

## 加载方式

1. 打开 Chrome。
2. 进入 `chrome://extensions/`
3. 打开右上角“开发者模式”。
4. 点击“加载已解压的扩展程序”。
5. 选择当前项目目录 `UrlSender`。

## 配置示例

后台地址：

```text
http://127.0.0.1:5998
```

中文输入法网址正则：

```text
^https://mail\.qq\.com/
^https://www\.baidu\.com/
```

英文输入法网址正则：

```text
^https://github\.com/
^https://stackoverflow\.com/
```
