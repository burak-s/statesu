# Statesu medya desteği — implementasyon planı

Durum: Aşama 1–6 uygulandı; üç inceleme turundaki tüm bulgular giderildi.

Üçüncü tur düzeltmeleri:

- `status_of()` hatasında cleanup sağlam dosyayı silmez: durum geçici
  olarak okunamazsa iş backoff'la retry'a döner, blind delete yapılmaz.
- Blog silme snapshot'ı medya ID'leriyle yetinmez; tam parmak izi
  (status + variant yolları) transaction içinde yeniden doğrulanır.
  Snapshot sonrası upload finalize olursa silme `RetrySnapshot` (409)
  ile iptal olur, işler rollback edilir; ikinci deneme temizliği tamamlar.
- APNG chunk-walk stride düzeltildi (8 + length + 4), overflow korumalı;
  gerçek IHDR + acTL fixtureleriyle statik dosyalar geçmeye devam eder.
- Sunucu secret'ları DB'ye yazılmaz: `media.storage_location` yalnız
  konum kimliğini (account/bucket/prefix/base) tutar. Aynı bucket
  kimliği güncel (rotasyonlu) config kimlik bilgileriyle temizlenir;
  Kimlik farklıysa "server bucket identity changed" hatasıyla görünür
  biçimde bekler ve yanlış anahtarla hiçbir şey silmez.
- Dashboard preview `img` kuralı component üstü `<style>` bloğuna
  taşındı; iç içe `<style>` öğesi kaldırıldı.
- Geç tamamlanan upload önce blog/editör eşleşmesiyle filtrelenir; medya
  listesine yalnız doğru blogda girer, Markdown ekleme davranışı korundu.

Dördüncü tur düzeltmeleri (storage değişiminde üç regresyon):

- Custom bağlantılı blog temizliği artık mühürlü bağlantı materyalini
  kullanır; blog payload'ındaki konum kimliği yalnız kayıt amaçlıdır ve
  çözümlemede sunucu bucket'ına asla yönlendirmez.
- Depolama snapshot anında çözülemese bile silme cascade'i yolları ve
  konum kimliğini kalıcı iş satırında korur; kayıt kaybı mümkün değil.
- Konfigürasyon beklerken (kimlik uyuşmazlığı, materyal yok) işler
  attempts tüketmez: kapsamlı backoff ile due'da kalır; config geri
  geldiğinde bakım sweep'i temizliği manuel adım olmadan tamamlar.

Beşinci tur düzeltmeleri (tekil silme + bekleme ilerlemesi):

- Tekil medya silme, depolama çözülemediğinde artık sahte 204 dönmez:
  satır aynı istekte `deleting` olur (public listelerden anında düşer)
  ve yollar kalıcı işte bekler; kimlik döndüğünde bakım sweep'i silmeyi
  kendiliğinden tamamlar. Blog orphan yoluyla aynı paylaşılan iş
  kurucusu (`unresolved_cleanup_job`) kullanılır.
- Configuration-wait işleri artık payload içinde ayrı bir `wait_count`
  ilerletir: gecikme 30 s → 60 s → 120 s şeklinde katlanarak 1 saat
  cap'ine kadar yükselir; attempts retry bütçesinden bağımsız kalır
  (`due_jobs` işi seçmeye devam eder).

Testler: Rust 151 (tekil silme beklemesi + backoff ilerlemesi testleri
dahil), web 48 unit + 11 Playwright; clippy temiz; build yeşil.
Kalan (opsiyonel, plan dışı): gerçek R2 smoke testi.

## 1. Amaç ve ürün sözleşmesi

Statesu Markdown içeriklerinde kullanıcı görsellerini destekleyecek. Dosyalar Cloudflare R2'de, metadata SQLite'ta tutulacak. Dashboard önizlemesi ve farklı sitelerdeki Statesu embed'leri ortak görüntüleme kurallarını kullanacak.

Kararlaştırılanlar:

- Görseller public; taslak yazılara yüklenmeleri bunu değiştirmez.
- Varsayılan depolama Statesu'nun R2 bucket'ı; kullanıcı kotasına tabidir.
- Kullanıcı blog bazında kendi R2 bağlantısını seçebilir.
- Markdown kalıcı HTTPS görsel adresleri içerir; süreli imzalı adresler içermez.
- Görsel genişliği kullanım yerine aittir; aynı medya farklı yazılarda farklı genişliklerde kullanılabilir.
- Görünüm tutarlılığı Statesu renderer/embed için geçerlidir. Rastgele Markdown motorlarında birebir görünüm garanti edilmez.
- Aynı container genişliği, tema ve renderer sürümünde tutarlı görünüm; farklı ekranlarda aynı responsive kurallar hedeflenir.
- Raw HTML kapalı kalır.
- Depolama bağlantısı değişikliği yalnızca yeni yüklemeleri etkiler; eski medyalar otomatik taşınmaz.
- Markdown'dan referans kaldırmak dosyayı otomatik silmez.

Başlangıç kapsamı: statik JPEG, PNG ve WebP. SVG, video, audio, genel dosya ekleri ve animasyon kapsam dışıdır. Animasyonlu dosyalar sessizce ilk kareye dönüştürülmez; açık hata ile reddedilir.

## 2. Mimari ilkeler ve kapsam kontrolü

- Mevcut auth, blog/post yönetimi, public delivery ve embed v1 sözleşmelerini koru. Değişiklikleri mümkün olduğunca eklemeli yap.
- Yeni modüller kurulabilir; gereksiz service/factory/manager katmanları kurulmaz. Mevcut repo/service/HTTP düzenini takip et.
- SOLID: HTTP handler'ları ince tut; medya yetkisi, kota ve yaşam döngüsü kurallarını medya modülünde topla. R2 ve codec ayrıntılarını çağıranlara sızdırma. Gerekli bağımlılıkları dışarıdan ver.
- DRY: Dashboard ve embed aynı Markdown renderer'ını kullansın. Doğrulama, kota ve storage kuralları tek yerde uygulansın. Farklı sorumluluklar yalnızca benzedikleri için zorla birleştirilmesin.
- KISS: Genel amaçlı çoklu-cloud framework'ü, plugin sistemi, ayrı mikroservis veya harici kuyruk kurma. Gerçek gereksinim yokken geleceğe yönelik abstraction ekleme.
- R2 erişimi için küçük, test edilebilir bir interface yeterli. İşleme ve silme güvenilirliği mevcut uygulama ve SQLite içindeki kalıcı işlerle sağlansın.
- Yeni bağımlılıkları somut gereksinimle gerekçelendir. Codec ve kriptografik imzalama gibi hassas işleri sırf bağımlılık azaltmak için sıfırdan yazma.
- Refactor'u entegrasyonun gerektirdiği alanlarla sınırla. İlgisiz kodu yeniden yazma.
- Mevcut test dosyalarını genişlet; her özellik için ayrı test dosyası oluşturma.
- Yeni frontend metinleri için mevcut dil/çeviri düzenini takip et.

Temel ilke: Yeni özellik kendi karmaşıklığını içinde taşımalı; mevcut codebase'in tamamına yaymamalı.

## 3. Mevcut entegrasyon noktaları

- `src/bootstrap.rs`: bağımlılıkların kurulması, uygulama state'i, dashboard CSP.
- `src/config.rs`: varsayılan R2 bağlantısı, şifreleme anahtarı ve kaynak limitleri.
- `src/http/mod.rs`: route'lar, body limitleri, timeout, CORS ve route sınıflandırması.
- `src/http/public.rs` ve `src/post/`: public yanıtlara referanslanan medya metadata'sının eklenmesi.
- `src/cli/`: upload/list/delete komutları.
- `web/src/embed/content.js`: mevcut marked → DOMPurify → URL politikası. Görseller şu anda kaldırılıyor.
- `web/src/embed/main.js`: mevcut v1 doğrudan host DOM'una render ediyor; font ve renk miras alıyor.
- `web/src/routes/dashboard/+page.svelte`: editör ve medya arayüzü entegrasyonu.
- Yeni medya sorumlulukları mevcut mimariye uygun `src/media/` ve HTTP handler modülünde tutulacak; dosya ayrımları gerçek sorumluluklara göre yapılacak.

Mevcut yazma timeout'u 5 saniye, CMS gövde limiti 256 KiB. Medya upload için ayrı sınırlar gerekir; mevcut JSON endpoint'leri gevşetilmez.

## 4. Depolama ve veri modeli

### Bucket düzeni

Varsayılan Statesu depolaması tek bucket kullanır. Kullanıcı bağlantılarında da yalnızca Statesu'ya ayrılmış prefix yönetilir:

```text
blogs/{blog_id}/{media_id}/source.jpg
blogs/{blog_id}/{media_id}/w640.webp
blogs/{blog_id}/{media_id}/w1280.webp
blogs/{blog_id}/{media_id}/w1920.webp
```

Prefix güvenlik sınırı değildir; yetkilendirme Statesu tarafından uygulanır. Kullanıcının bucket'ındaki diğer dosyalara dokunulmaz.

### Tablolar

Yeni migration ile:

- `storage_connections`: blog, R2 account ID, bucket, prefix, public base URL, şifreli erişim bilgileri ve bağlantı durumu.
- `media`: sahip, blog, yükleme sırasında seçilen bağlantı, durum, kaynak ölçüleri, toplam saklanan byte ve zamanlar.
- `media_variants`: kaynak/türev object key, format, ölçüler ve byte bilgileri.
- `media_jobs`: yeniden denenmesi gereken işleme/temizlik işleri; deneme sayısı ve sonraki çalışma zamanı.

Blogun aktif depolama tercihi tutulur. Statesu varsayılanı sunucu config'inden gelir; sunucu secret'ları kullanıcı bağlantısı gibi DB'ye kopyalanmaz. Varsayılan bağlantı değişiklikleri de eski medyaların konumunu sessizce değiştirmemelidir; mevcut dosya adresi ve konum bilgileri korunur.

Her medya yüklendiği bağlantıya bağlı kalır. Referanslanan bağlantı doğrudan silinemez. Anahtar yenilemek, bağlantının public adresini veya mevcut medyaların yerini değiştirmekten ayrı bir işlemdir.

## 5. R2 bağlantısı ve secret güvenliği

Kullanıcı bağlantısı için:

- Cloudflare account ID
- Bucket adı
- Bucket'a gerekli yetkilerle sınırlandırılmış access key ID/secret
- Public HTTPS medya base URL

Ana hesap anahtarı istenmez. Secret'lar ayrı storage encryption key ile şifrelenir; yanıtlarda, loglarda ve hata metinlerinde gösterilmez. Şifreleme anahtarı yenileme ve yedekleme gereksinimleri belgelenir.

R2 endpoint'i doğrulanmış account ID'den oluşturulur; kullanıcıdan rastgele S3 endpoint alınmaz.

Bağlantı doğrulama:

1. Blog sahipliğini kontrol et.
2. Statesu prefix'ine küçük test dosyası yaz.
3. Public medya adresinden içeriğin erişilebilirliğini doğrula.
4. Test dosyasını temizle; başarısız temizliği takip et.
5. Doğrulama başarılı olmadan aktif bağlantıyı değiştirme.

Public URL doğrulaması SSRF yüzeyidir: private/loopback/link-local adresleri, DNS çözümleme ve bağlantı zamanı kontrolleri, redirect politikası, port, süre ve yanıt boyutu sınırlanır. Arbitrary public URL'den sunucu tarafında görsel import bu kapsama alınmaz.

R2 erişim yetkisi ve public domain kurulumu farklı adımlar olarak arayüzde açıklanır.

## 6. Upload, görsel işleme ve kota

### Akış

```text
Yetki kontrolü
→ upload sınırları
→ atomik kota rezervasyonu
→ doğrulama/decode
→ metadata temizleme ve yön düzeltme
→ temiz kaynak ve responsive türevler
→ R2'ye yazma
→ metadata/kota kesinleştirme
→ ready
```

Başlangıçta backend üzerinden upload kullanılır. Doğrudan R2 upload bu aşamanın kapsamı değildir; ileride eklenirse aynı doğrulama ve yaşam döngüsü kurallarını kullanır.

### Kontroller

- Gövde tüketilirken byte limiti; sınırsız buffering yok.
- Gerçek format doğrulaması; uzantı ve istemci MIME değerine güvenilmez.
- Genişlik, yükseklik, toplam piksel ve decoder kaynak limitleri.
- Animasyonun açık şekilde reddedilmesi.
- EXIF yönünün uygulanması; hassas metadata'nın temizlenmesi.
- Kullanıcı dosya adından bağımsız UUID tabanlı object key.
- Görsel işleme için sınırlı blocking worker ve eşzamanlı upload sayısı.

Temizlenmiş kaynak yeniden encode edilir; ham dosya ayrıca saklanmaz. Bu davranış ve olası kalite/renk profili etkileri belgelenir ve örneklerle doğrulanır.

### Türevler

Başlangıç adayı 640/1280/1920 px genişliklerdir. Kaynaktan büyük sürüm üretilmez; gereksiz aynı ölçüde türevler oluşturulmaz. Kesin ölçü ve kalite değerleri örnek içeriklerle ölçülerek sabitlenir. Kontrolsüz on-demand dönüşüm endpoint'i açılmaz.

### Kota

- Statesu depolama kotası kullanıcı toplamında hesaplanır.
- Temiz kaynak ve bütün türevler kotaya dahildir.
- Eşzamanlı istekler atomik rezervasyonla kotayı aşamaz.
- Rezervasyon gerçek çıktı büyüklüğüne göre güvenli biçimde kesinleştirilir; yetersizse yayınlama yapılmaz ve temizlik yürütülür.
- Başarısız işlerde rezervasyon serbest bırakılır; R2'de kalan dosyalar temizlik tamamlanana kadar takip edilir.
- Kullanıcının R2'sinde Statesu depolama kotası uygulanmaz; dosya, trafik/işlem kötüye kullanım ve eşzamanlılık sınırları uygulanır.

Dosya, kota, timeout ve concurrency varsayılanları config'den ayarlanır; kesin değerler ölçümle belirlenir.

### Güvenilirlik

İşleme durumu yalnızca HTTP isteğinin belleğinde tutulmaz. Yeniden başlatma, timeout, istemci bağlantısının kopması ve kısmi R2 hatalarında DB kayıtlarından güvenli toparlama yapılır. Tekrar çalıştırma idempotent olur. Bekleyen işlerde sınırlı retry/backoff ve hata görünürlüğü sağlanır. SQLite transaction'ı ağ veya codec çalışması boyunca açık tutulmaz.

## 7. Yönetim ve public delivery

Önerilen yönetim route'ları:

```text
GET/POST     /blogs/{blog_id}/storage-connections
PATCH/DELETE /blogs/{blog_id}/storage-connections/{connection_id}
PUT          /blogs/{blog_id}/media-storage
POST/GET     /blogs/{blog_id}/media
GET/DELETE   /blogs/{blog_id}/media/{media_id}
```

Mevcut sahiplik ve session/PAT politikaları korunur. Storage secret yönetimi dashboard session'ıyla sınırlandırılır; media upload/list/delete CLI token'larıyla kullanılabilir. Listelemelerde mevcut pagination düzeni takip edilir. Yeni route'lar mevcut CORS, CSRF/origin, body limit ve timeout sınıflandırmalarına açıkça dahil edilir.

Public post yanıtları Markdown'ı değiştirmeden, yalnızca referanslanan hazır Statesu medyalarının metadata'sını eklemeli olarak sunar:

- Canonical kaynak URL
- Kaynak ölçüleri
- Responsive türev URL/ölçüleri
- İnceleme için yüksek çözünürlüklü sürüm

Liste yanıtlarında medya kayıtları toplu getirilir; N+1 sorgu ve her görsel için ek HTTP isteği önlenir. Metadata credential, bucket erişim secret'ı veya yönetim bilgisi içermez.

Harici görseller normal gösterilebilir; sunucu bu URL'lerden dosya indirmez. Statesu optimizasyonu yalnızca bilinen medya kayıtlarında uygulanır.

Markdown'a gerçek public kaynak adresi yazılır. Metadata bulunmazsa temel görsel yine çalışır. Kullanıcının kendi domain'ini kullanması teşvik edilir. Domain değişen taşımalarda URL güncellemesi gerektiği ve Statesu domain'inin kalıcılık politikasına bağlı olduğu açıkça belirtilir.

## 8. Markdown ve ortak renderer

Standart görsel:

```md
![Açıklama](https://media.example.com/image.webp)
```

Boyut uzantısı:

```md
![Açıklama](https://media.example.com/image.webp){width=640}
```

- marked parser uzantısıyla işlenir; Markdown üzerinde genel regex dönüşümü yapılmaz.
- Yalnızca tanımlanmış sınırlar içindeki pozitif genişlik kabul edilir.
- Kod blokları, inline code, linkli görseller ve diğer Markdown yapıları korunur.
- Keyfi style, class, event attribute veya raw HTML kabul edilmez.
- Hatalı uzantının fallback davranışı belgelenir ve test edilir.
- Genişlik yoksa doğal boyut, en fazla kapsayıcı genişliği kullanılır.
- İstenen genişlik mobilde taşmaya neden olmaz; en-boy oranı korunur.

`web/src/embed/content.js` güvenlik zinciri korunur: parser → DOMPurify → URL/attribute politikası. `img`, alt metin, güvenli kaynak URL'leri, ölçüler ve responsive attribute'lar kontrollü üretilir. `srcset` yalnızca doğrulanmış metadata'dan oluşturulur. Medya için güvenli protokol politikası uygulanır; production HTTPS, gerekirse yalnızca geliştirmeye özel localhost istisnası.

Dashboard önizlemesi ve embed aynı renderer ve görsel kurallarını kullanır. Dashboard CSP'si ve host sayfalara gereken `img-src`/script/style politikaları tasarlanır; keyfi origin'lere ağ isteği açan geniş CSP gevşetmeleri yapılmaz.

## 9. Embed v2 ve lightbox

Mevcut v1 davranışını sessizce değiştirmemek için medya deneyimi `/embed/v2.js` altında sunulur. v1 korunur.

- Shadow DOM ile host CSS'inden izolasyon.
- Açık tema değişkenleri ve tanımlı tipografi/renk varsayılanları.
- Mevcut post/feed modlarında ortak medya davranışı.
- Aynı container/tema/sürüm koşullarında tutarlı görünüm.
- Sürüm değişiklikleri ve geçiş adımları belgelenir; tam sürüm pinleme ihtiyacı yayın politikasıyla netleştirilir.

Lightbox:

- Link içinde olmayan görsellerde etkin; linkli görselin hedefi öncelikli.
- Büyük sürüm açılışta yüklenir; başlangıçta gereksiz yüksek çözünürlük indirilmez.
- Zoom, pan ve mobil pinch; klavye ile kullanılabilir eşdeğer kontroller.
- Esc, odak geri dönüşü, odak yönetimi, ekran okuyucu etiketleri ve reduced-motion.
- Embed ayarıyla kapatılabilir.
- Yükleme/destek hatasında kaynak görsele erişim yolu.
- Native dialog/top-layer yaklaşımıyla host overflow ve z-index etkilerinin önlenmesi.
- Kapanışta ve yeniden render'da event/listener/gesture kaynaklarının temizlenmesi.

## 10. Dashboard ve CLI

Dashboard:

- Dosya seçme, drag-and-drop ve clipboard upload.
- Upload ilerleme/durum ve anlamlı hata mesajları.
- Alt metin ve genişlik düzenleme; doğrudan Markdown'a yazma.
- Ortak renderer ile önizleme.
- Medya listesi, kota ve silme uyarıları.
- R2 bağlantısı doğrulama, seçme ve anahtar yenileme.
- Upload sırasında yazı değişirse yanlış editöre eklememe; doğru imleç konumu ve istek yaşam döngüsü yönetimi.
- Public taslak görseli ve domain kalıcılığı hakkında açık bilgilendirme.

CLI, mevcut komut yapısına uygun `media upload`, `media list`, `media delete` sağlar. Upload Markdown satırı ve mevcut çıktı düzenine uygun makine-okunabilir çıktı sunar. Yerel Markdown içindeki tüm dosyaları otomatik keşfetme/yükleme kapsam dışıdır.

## 11. Silme ve işletim

- Referans kaldırmak dosya silmez.
- Açık medya silmede mevcut referanslar ve kırılma riski gösterilir.
- Referans tespitinin kapsamı sahip olunan içeriktir; dış sitelerde kopyalanmış referansların bilinemeyeceği açıklanır.
- Silme önce kalıcı iş olarak kaydedilir; DB/object key bilgileri temizlik bitmeden kaybolmaz.
- Blog/hesap silme DB cascade'inin storage temizliği bilgisini yok etmesine izin vermez.
- Başarısız R2 silmeleri sınırlı retry ile takip edilir.
- Kullanıcının iptal ettiği credentials nedeniyle temizlenemeyen dosyalar görünür biçimde raporlanır.
- CDN cache nedeniyle silmenin anında erişilemezlik garantisi vermediği belgelenir.
- R2 erişim kapsamı, public domain kurulumu, encryption key yedeği ve kota politikası mevcut dokümanlara eklenir.

## 12. Test ve doğrulama

Mevcut `tests/cms_api.rs`, `tests/public_api.rs`, `tests/http_contract.rs` ve test helpers genişletilir:

- Sahiplik, session/PAT, origin ve route sınıflandırması.
- Secret sızıntısı, SSRF ve bağlantı doğrulama hataları.
- Format sahteciliği, bozuk dosya, animasyon ve aşırı piksel/bellek kullanımı.
- Eşzamanlı kota rezervasyonu ve rezervasyonların geri alınması.
- Kısmi R2 hataları, idempotency ve restart sonrası toparlama.
- Bağlantı değişikliği, credential yenileme ve silme.
- Public metadata'nın yalnızca ilgili hazır medyaları içermesi.
- Blog/hesap silmede güvenilir object cleanup.

Mevcut `web/tests/embed.test.js`, `embed.e2e.test.js` ve browser testleri genişletilir:

- Boyut parser'ı, kod blokları ve XSS payload'ları.
- Responsive kaynak seçimi, aspect ratio ve mobil taşma.
- Agresif host CSS altında izolasyon.
- Lightbox keyboard/focus, mobil gesture ve kapanış temizliği.
- Linkli görsel önceliği, broken image ve metadata fallback.
- Dashboard upload yarışları ve önizleme eşleşmesi.
- v1 geriye uyumluluğu.

Normal testler dış ağa bağımlı olmaz. Gerçek R2 ile opt-in smoke test ayrıca yapılır. Codec, byte, kalite ve bellek sınırları örnek görsellerle ölçülür.

## 13. Uygulama sırası

1. Veri modeli, config, secret yönetimi ve test edilebilir R2 interface'i.
2. Bağlantı doğrulama, güvenli upload, dönüştürme, kota ve temizlik.
3. Public metadata ve ortak Markdown renderer.
4. Embed v2, stil izolasyonu ve lightbox.
5. Dashboard, CLI ve uçtan uca doğrulama.
6. Mevcut docs sayfaları, README, `.env.example` ve deployment ayarları.

Her aşamada testler geliştirmeyle birlikte eklenir; sona bırakılmaz.

İlk dikey kabul kriteri: Varsayılan R2'ye yüklenen görselin dashboard'da Markdown'a eklenmesi, farklı iki sitede embed v2 ile aynı kurallarla görüntülenmesi ve lightbox'ta açılması. Aynı akış kullanıcıya ait R2 bağlantısıyla da doğrulanır.

## 14. Her aşama için tamamlanma ölçütü

1. İlgili özellik ve hata yolu testleri geçiyor.
2. Mevcut testler ve sözleşmeler korunuyor.
3. Gereksiz abstraction, tekrar veya kapsam dışı refactor eklenmemiş.
4. Kaynak kullanımı sınırlandırılmış; cleanup ve retry yolları doğrulanmış.
5. Secret'lar ve public metadata güvenlik kontrolünden geçmiş.
6. Değişiklik yalnızca ilgili aşamanın kapsamını içeriyor.

Implementasyon sırasında ölçümle netleştirilecekler: dosya/piksel/kota varsayılanları, codec kalite değerleri, bounded worker sayısı, retry sınırları ve embed sürüm pinleme politikası. Bunlar ürünün kararlaştırılmış temel davranışını değiştirmeden belirlenir.
