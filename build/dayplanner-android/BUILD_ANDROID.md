# Сборка Android

Локальный проект: `E:\RustDnevnik\build\dayplanner-android`. Версия приложения — 0.2.0.

```powershell
.\build-apk.ps1              # подписанный release: arm64 + armv7
.\build-apk.ps1 -DebugBuild  # debug
.\build-apk.ps1 -AllAbis     # также x86 / x86_64
```

Скрипт находит JDK 17, SDK в `%LOCALAPPDATA%\Android\Sdk`, NDK 27,
Rust GNU и WinLibs/MinGW. Системные переменные не меняются.
Готовый APK: `../../dist/dayplanner-android-<версия>.apk`.
compileSdk/targetSdk = 37, minSdk = 24, плагин использует compileSdk = 37.

## Подпись

- Ключ: `keys/dayplanner-release.jks` (сохранён из прежней рабочей копии).
- Пароли, alias и путь к ключу: `app/src-tauri/gen/android/keystore.properties`.
- Gradle читает этот файл в `app/src-tauri/gen/android/app/build.gradle.kts`.
- Эти файлы секретные; не включайте их в публикацию исходников.
- Не удаляйте ключ: он нужен для обновления уже установленного приложения.

Сгенерированная папка `app/src-tauri/gen/` сохранена вместе с настройками подписи.
После `tauri android init` на чистом клоне потребуется восстановить `keystore.properties`
и блок `signingConfigs.release` в Gradle. Скрипт предупреждает о регенерации,
но сам пароли и конфигурацию подписи не создаёт.

## Проверка

```powershell
& "$env:LOCALAPPDATA\Android\Sdk\build-tools\37.0.0\apksigner.bat" verify ..\..\dist\dayplanner-android-0.2.0.apk
& "$env:LOCALAPPDATA\Android\Sdk\platform-tools\adb.exe" install -r ..\..\dist\dayplanner-android-0.2.0.apk
```

После установки проверьте панель на своём телефоне: жесты/три системные кнопки,
поворот, вырез экрана и четыре темы. Автоматические проверки в браузере не заменяют
проверку конкретной прошивки и Android System WebView.
