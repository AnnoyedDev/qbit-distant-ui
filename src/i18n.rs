use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Debug)]
pub enum Lang {
    #[default]
    Fr,
    En,
    Ja,
}

impl Lang {
    pub const ALL: [Lang; 3] = [Lang::Fr, Lang::En, Lang::Ja];

    pub fn name(self) -> &'static str {
        match self {
            Lang::Fr => "Français",
            Lang::En => "English",
            Lang::Ja => "日本語",
        }
    }

    pub fn detect() -> Lang {
        let v = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
            .unwrap_or_default();
        if v.starts_with("fr") {
            Lang::Fr
        } else if v.starts_with("ja") {
            Lang::Ja
        } else {
            Lang::En
        }
    }

    pub fn t(self, key: &'static str) -> &'static str {
        T.iter().find(|(k, _)| *k == key).map(|(_, v)| v[self as usize]).unwrap_or(key)
    }

    pub fn msg(self, s: &str) -> String {
        T.iter().find(|(k, _)| *k == s).map(|(_, v)| v[self as usize].to_string()).unwrap_or_else(|| s.to_string())
    }
}

const T: &[(&str, [&str; 3])] = &[
    ("connect", ["Connexion", "Connect", "接続"]),
    ("disconnect", ["Déconnexion", "Disconnect", "切断"]),
    ("settings", ["Paramètres", "Settings", "設定"]),
    ("add", ["Ajouter", "Add", "追加"]),
    ("open_file", ["Ouvrir un .torrent", "Open a .torrent", ".torrentを開く"]),
    ("add_hint", ["Collez un lien magnet ou une URL… (ou glissez un .torrent ici)", "Paste a magnet link or URL… (or drop a .torrent here)", "マグネットリンクやURLを貼り付け…（.torrentのドロップも可）"]),
    ("filter", ["Filtrer", "Search", "検索"]),
    ("no_torrent", ["Rien à afficher pour le moment", "Nothing to show yet", "表示できるトレントはありません"]),
    ("loading", ["Récupération de la liste des torrents…", "Fetching your torrents…", "トレント一覧を取得しています…"]),
    ("no_server", ["Aucun serveur enregistré : ajoutez-en un dans les paramètres.", "No server saved yet — add one in Settings.", "サーバーが登録されていません。設定から追加してください。"]),
    ("delete_q", ["Supprimer ce torrent ?", "Remove this torrent?", "このトレントを削除しますか？"]),
    ("delete", ["Supprimer", "Remove", "削除"]),
    ("delete_files", ["Supprimer + fichiers", "Remove + files", "ファイルごと削除"]),
    ("cancel", ["Annuler", "Cancel", "キャンセル"]),
    ("pause", ["Pause", "Pause", "一時停止"]),
    ("resume", ["Reprendre", "Resume", "再開"]),
    ("st_dl", ["Téléchargement", "Downloading", "ダウンロード中"]),
    ("st_seed", ["Partage", "Seeding", "シード中"]),
    ("st_stalled", ["En attente de pairs", "Waiting for peers", "ピアを待機中"]),
    ("st_meta", ["Métadonnées", "Fetching metadata", "メタデータ取得中"]),
    ("st_queued", ["En file", "Queued", "待機中"]),
    ("st_check", ["Vérification", "Checking", "検証中"]),
    ("st_moving", ["Déplacement", "Moving", "移動中"]),
    ("st_error", ["Erreur", "Error", "エラー"]),
    ("st_done", ["Terminé", "Completed", "完了"]),
    ("st_paused", ["En pause", "Paused", "停止中"]),
    ("st_unknown", ["Inconnu", "Unknown", "不明"]),
    ("f_all", ["Tous", "All", "すべて"]),
    ("f_dl", ["En cours", "Downloading", "ダウンロード中"]),
    ("f_seed", ["En partage", "Seeding", "シード中"]),
    ("f_done", ["Terminés", "Completed", "完了"]),
    ("f_paused", ["En pause", "Paused", "停止中"]),
    ("f_active", ["Actifs", "Active", "アクティブ"]),
    ("f_error", ["Erreurs", "Errors", "エラー"]),
    ("cat_all", ["Toutes catégories", "All categories", "全カテゴリ"]),
    ("sort_added", ["Date d'ajout", "Date added", "追加日"]),
    ("sort_name", ["Nom", "Name", "名前"]),
    ("sort_size", ["Taille", "Size", "サイズ"]),
    ("sort_progress", ["Progression", "Progress", "進捗"]),
    ("sort_speed", ["Vitesse", "Speed", "速度"]),
    ("sort_ratio", ["Ratio", "Ratio", "共有比"]),
    ("sort", ["Trier par", "Sort by", "並べ替え"]),
    ("sort_dir", ["Inverser l'ordre", "Flip order", "並び順を反転"]),
    ("start_all", ["Tout reprendre", "Resume all", "すべて再開"]),
    ("stop_all", ["Tout mettre en pause", "Pause all", "すべて停止"]),
    ("alt_speed", ["Limites alternatives (mode tortue)", "Alternative speed limits (turtle mode)", "代替速度制限（タートルモード）"]),
    ("details", ["Détails", "Details", "詳細"]),
    ("recheck", ["Revérifier les données", "Force recheck", "データを再チェック"]),
    ("reannounce", ["Réannoncer aux trackers", "Reannounce to trackers", "トラッカーに再アナウンス"]),
    ("copy_magnet", ["Copier le lien magnet", "Copy magnet link", "マグネットリンクをコピー"]),
    ("move", ["Déplacer…", "Move…", "移動…"]),
    ("add_title", ["Ajouter un torrent", "Add a torrent", "トレントを追加"]),
    ("save_to", ["Enregistrer dans", "Save to", "保存先"]),
    ("browse", ["Parcourir", "Browse", "参照"]),
    ("category", ["Catégorie", "Category", "カテゴリ"]),
    ("none", ["(aucune)", "(none)", "（なし）"]),
    ("start_now", ["Démarrer tout de suite", "Start right away", "すぐに開始する"]),
    ("pick_after", ["Décochez pour choisir les fichiers à télécharger avant de lancer.", "Untick to pick which files to grab before it starts.", "チェックを外すと、開始前にダウンロードするファイルを選べます。"]),
    ("queued_more", ["en attente", "waiting", "件待機中"]),
    ("explorer_title", ["Dossiers du serveur", "Folders on the server", "サーバー上のフォルダー"]),
    ("parent", ["Dossier parent", "Up one level", "上の階層へ"]),
    ("choose", ["Choisir ce dossier", "Use this folder", "このフォルダーにする"]),
    ("explorer_unavailable", ["Navigation impossible ici (il faut qBittorrent 5.0 ou plus) : tapez le chemin à la main.", "Folder browsing isn't available (needs qBittorrent 5.0+) — just type the path.", "フォルダーを参照できません（qBittorrent 5.0以降が必要です）。パスを直接入力してください。"]),
    ("empty_dir", ["Aucun sous-dossier", "No subfolders", "サブフォルダーはありません"]),
    ("files", ["Fichiers", "Files", "ファイル"]),
    ("waiting_meta", ["Les métadonnées arrivent, la liste des fichiers s'affichera ensuite…", "Waiting for metadata — the file list will show up shortly…", "メタデータを待っています。まもなくファイル一覧が表示されます…"]),
    ("select_all", ["Tout cocher", "Select all", "すべて選択"]),
    ("select_none", ["Tout décocher", "Select none", "選択解除"]),
    ("location", ["Emplacement", "Location", "保存場所"]),
    ("size", ["Taille", "Size", "サイズ"]),
    ("state", ["État", "Status", "状態"]),
    ("tab_general", ["Général", "General", "一般"]),
    ("tab_servers", ["Serveurs", "Servers", "サーバー"]),
    ("tab_watch", ["Dossiers surveillés", "Watched folders", "監視フォルダー"]),
    ("tab_remote", ["Réglages du serveur", "Server options", "サーバー側の設定"]),
    ("language", ["Langue", "Language", "言語"]),
    ("theme", ["Thème", "Theme", "テーマ"]),
    ("ask_on_add", ["Me demander les options avant chaque ajout", "Ask me for options before each add", "追加のたびにオプションを確認する"]),
    ("associate", ["Ouvrir les liens magnet et les .torrent avec cette application", "Open magnet links and .torrent files with this app", "マグネットリンクと.torrentをこのアプリで開く"]),
    ("associate_ok", ["C'est fait : votre navigateur enverra désormais les torrents ici, même si l'application est déjà ouverte.", "Done — your browser will now hand torrents to this app, even while it's already running.", "設定しました。アプリの起動中でも、ブラウザーのトレントはこのアプリで開かれます。"]),
    ("name", ["Nom", "Name", "名前"]),
    ("url", ["Adresse", "URL", "URL"]),
    ("user", ["Utilisateur", "Username", "ユーザー名"]),
    ("password", ["Mot de passe", "Password", "パスワード"]),
    ("default", ["Par défaut", "Default", "デフォルト"]),
    ("default_hint", ["Connexion automatique à ce serveur au démarrage", "Connect to this server automatically at startup", "起動時にこのサーバーへ自動接続します"]),
    ("add_server", ["Ajouter un serveur", "Add a server", "サーバーを追加"]),
    ("watch_help", ["Tout .torrent qui apparaît dans un de ces dossiers part vers le serveur connecté, puis il est supprimé du dossier.", "Any .torrent that shows up in one of these folders is sent to the connected server, then deleted from the folder.", "これらのフォルダーに.torrentが現れると、接続中のサーバーへ送信してからフォルダーから削除します。"]),
    ("local_folder", ["Dossier local", "Local folder", "ローカルフォルダー"]),
    ("remote_folder", ["Dossier distant", "Remote folder", "保存先（サーバー）"]),
    ("add_watch", ["Ajouter un dossier", "Add a folder", "フォルダーを追加"]),
    ("enabled", ["Actif", "On", "有効"]),
    ("watch_sent", ["Envoyé depuis un dossier surveillé :", "Picked up from a watched folder:", "監視フォルダーから送信しました："]),
    ("added_ok", ["Torrent ajouté.", "Torrent added.", "トレントを追加しました。"]),
    ("dl_limit", ["Limite de téléchargement (Kio/s, 0 = illimité)", "Download limit (KiB/s, 0 = unlimited)", "ダウンロード制限（KiB/s、0で無制限）"]),
    ("up_limit", ["Limite d'envoi (Kio/s, 0 = illimité)", "Upload limit (KiB/s, 0 = unlimited)", "アップロード制限（KiB/s、0で無制限）"]),
    ("ratio_limit", ["Quand le ratio atteint", "When the ratio reaches", "共有比がこの値に達したとき"]),
    ("seed_limit", ["Quand le partage dure (minutes)", "When seeding time reaches (minutes)", "シード時間がこの値に達したとき（分）"]),
    ("then_action", ["Ensuite", "Then", "そのあと"]),
    ("act_stop", ["Mettre en pause", "Pause it", "停止する"]),
    ("act_remove", ["Retirer le torrent", "Remove the torrent", "トレントを削除する"]),
    ("act_remove_files", ["Retirer le torrent et ses fichiers", "Remove the torrent and its files", "トレントとファイルを削除する"]),
    ("act_superseed", ["Passer en super-seeding", "Switch to super seeding", "スーパーシードに切り替える"]),
    ("auto_remove_help", ["Ces règles s'appliquent sur le serveur, même quand l'application est fermée.", "These rules run on the server itself, even when this app is closed.", "これらのルールはサーバー側で動作し、アプリを閉じていても適用されます。"]),
    ("apply", ["Appliquer", "Apply", "適用"]),
    ("saved", ["Réglages envoyés au serveur.", "Settings sent to the server.", "サーバーに設定を送信しました。"]),
    ("connect_first", ["Connectez-vous d'abord pour modifier ces réglages.", "Connect first to change these options.", "変更するには、まずサーバーに接続してください。"]),
    ("notif_done", ["Téléchargement terminé", "Download finished", "ダウンロードが完了しました"]),
    ("notify_done", ["Me prévenir par une notification quand un téléchargement se termine", "Send me a notification when a download finishes", "ダウンロードが完了したら通知する"]),
    ("back", ["Retour aux torrents", "Back to torrents", "トレント一覧に戻る"]),
    ("free_space", ["libres", "free", "空き"]),
    ("info", ["Infos", "Info", "情報"]),
    ("trackers", ["Trackers", "Trackers", "トラッカー"]),
    ("tr_0", ["Désactivé", "Disabled", "無効"]),
    ("tr_1", ["Pas encore contacté", "Not contacted yet", "未接続"]),
    ("tr_2", ["Fonctionne", "Working", "稼働中"]),
    ("tr_3", ["Mise à jour…", "Updating…", "更新中…"]),
    ("tr_4", ["Ne répond pas", "Not working", "応答なし"]),
    ("no_tracker", ["Aucun tracker (DHT / PeX uniquement)", "No trackers (DHT / PeX only)", "トラッカーなし（DHT／PeXのみ）"]),
    ("seeds_peers", ["Sources / pairs", "Seeds / peers", "シード／ピア"]),
    ("force_start", ["Forcer le démarrage (passe devant la file d'attente)", "Force start (jump the queue)", "強制開始（キューを無視）"]),
    ("sequential", ["Téléchargement séquentiel (pratique pour regarder pendant le téléchargement)", "Sequential download (handy for watching while it downloads)", "順次ダウンロード（ダウンロードしながら再生するのに便利）"]),
    ("first_last", ["Priorité au premier et au dernier morceau", "Prioritize first and last pieces", "最初と最後のピースを優先"]),
    ("limits", ["Limites pour ce torrent (Kio/s, 0 = illimité)", "Limits for this torrent (KiB/s, 0 = unlimited)", "このトレントの速度制限（KiB/s、0で無制限）"]),
    ("dl_short", ["Réception", "Download", "ダウンロード"]),
    ("up_short", ["Envoi", "Upload", "アップロード"]),
    ("rename", ["Renommer", "Rename", "名前を変更"]),
    ("save", ["Valider", "Save", "保存"]),
    ("shortcuts", ["Raccourcis : Ctrl+O ouvrir un .torrent · Ctrl+V coller un lien magnet · Ctrl+F rechercher · Ctrl+A tout sélectionner · Espace pause/reprise · Entrée détails · F5 actualiser · Suppr supprimer la sélection · Maj+clic sélectionner une plage · Échap fermer", "Shortcuts: Ctrl+O open a .torrent · Ctrl+V paste a magnet link · Ctrl+F search · Ctrl+A select all · Space pause/resume · Enter details · F5 refresh · Del remove selected · Shift+click select a range · Esc close", "ショートカット：Ctrl+O .torrentを開く・Ctrl+V マグネットリンクを貼り付け・Ctrl+F 検索・Ctrl+A すべて選択・Space 一時停止／再開・Enter 詳細・F5 更新・Del 選択項目を削除・Shift+クリック 範囲選択・Esc 閉じる"]),
    ("selected", ["sélectionné(s)", "selected", "件選択中"]),
    ("select_visible", ["Tout sélectionner", "Select all", "すべて選択"]),
    ("clear_selection", ["Désélectionner", "Clear selection", "選択を解除"]),
    ("set_category", ["Changer la catégorie", "Set category", "カテゴリを変更"]),
    ("n_torrents", ["torrents", "torrents", "件のトレント"]),
    ("add_trackers", ["Ajouter les trackers", "Add trackers", "トラッカーを追加"]),
    ("trackers_hint", ["udp://… ou https://… (une adresse par ligne)", "udp://… or https://… (one per line)", "udp://… または https://…（1行に1つ）"]),
    ("remove_tracker", ["Retirer ce tracker", "Remove this tracker", "このトラッカーを削除"]),
    ("tray_show", ["Afficher qBit-Distant-UI", "Show qBit-Distant-UI", "qBit-Distant-UIを表示"]),
    ("quit", ["Quitter", "Quit", "終了"]),
    ("close_to_tray", ["Garder l'application dans la zone de notification quand je ferme la fenêtre", "Keep running in the system tray when I close the window", "ウィンドウを閉じてもシステムトレイで動作を続ける"]),
    ("tray_unavailable", ["Pas de zone de notification sur ce bureau : fermer la fenêtre quittera l'application.", "No system tray on this desktop — closing the window will quit the app.", "このデスクトップではシステムトレイが使えません。ウィンドウを閉じるとアプリは終了します。"]),
    ("tags", ["Étiquettes", "Tags", "タグ"]),
    ("tag_all", ["Toutes étiquettes", "All tags", "全タグ"]),
    ("add_tag", ["Ajouter cette étiquette", "Add this tag", "このタグを付ける"]),
    ("remove_tag", ["Retirer cette étiquette", "Remove this tag", "このタグを外す"]),
    ("new_tag", ["Nouvelle étiquette…", "New tag…", "新しいタグ…"]),
    ("queue", ["File d'attente", "Queue", "キュー"]),
    ("queue_top", ["Tout en haut de la file", "Move to top", "先頭へ移動"]),
    ("queue_up", ["Monter d'un cran", "Move up", "ひとつ上へ"]),
    ("queue_down", ["Descendre d'un cran", "Move down", "ひとつ下へ"]),
    ("queue_bottom", ["Tout en bas de la file", "Move to bottom", "末尾へ移動"]),
    ("err_queue", ["La file d'attente est désactivée sur le serveur (Options › BitTorrent).", "Queueing is turned off on the server (Options › BitTorrent).", "サーバー側でキューが無効になっています（オプション › BitTorrent）。"]),
    ("global_limits", ["Limites globales (Kio/s, 0 = illimité)", "Global speed limits (KiB/s, 0 = unlimited)", "全体の速度制限（KiB/s、0で無制限）"]),
    ("copy_hash", ["Copier le hash", "Copy info hash", "ハッシュをコピー"]),
    ("peers", ["Pairs", "Peers", "ピア"]),
    ("no_peer", ["Aucun pair connecté pour le moment", "No peers connected right now", "接続中のピアはありません"]),
    ("address", ["Adresse", "Address", "アドレス"]),
    ("client", ["Client", "Client", "クライアント"]),
    ("country", ["Pays", "Country", "国"]),
    ("speed_history", ["Vitesses des 3 dernières minutes", "Speeds over the last 3 minutes", "直近3分間の速度"]),
    ("all_servers", ["Tous les serveurs", "All servers", "すべてのサーバー"]),
    ("current", ["(connecté)", "(connected)", "（接続中）"]),
    ("tab_categories", ["Catégories et étiquettes", "Categories & tags", "カテゴリとタグ"]),
    ("categories", ["Catégories", "Categories", "カテゴリ"]),
    ("cat_path", ["Dossier par défaut (vide = celui du serveur)", "Default folder (empty = server default)", "既定の保存先（空欄でサーバーの既定）"]),
    ("new_category", ["Nouvelle catégorie", "New category", "新しいカテゴリ"]),
    ("create", ["Créer", "Create", "作成"]),
    ("err_auth", ["Identifiants refusés : vérifiez l'utilisateur et le mot de passe.", "Login rejected — double-check the username and password.", "ログインできませんでした。ユーザー名とパスワードを確認してください。"]),
    ("err_forbidden", ["Accès refusé (403) : IP bannie par qBittorrent ou bloquée par un proxy / Cloudflare.", "Access denied (403): banned by qBittorrent or blocked by a proxy / Cloudflare.", "アクセスが拒否されました（403）。qBittorrentによるBAN、またはプロキシ／Cloudflareによるブロックです。"]),
    ("err_notfound", ["Introuvable (404) : vérifiez l'adresse du serveur.", "Not found (404) — check the server address.", "見つかりません（404）。サーバーのアドレスを確認してください。"]),
    ("err_url", ["Adresse invalide : elle doit commencer par http:// ou https://", "Invalid address — it has to start with http:// or https://", "アドレスが不正です。http:// か https:// で始めてください。"]),
    ("err_host", ["Hôte introuvable : l'adresse semble incorrecte.", "Can't find that host — the address looks wrong.", "ホストが見つかりません。アドレスが間違っている可能性があります。"]),
    ("err_timeout", ["Le serveur ne répond pas (délai dépassé).", "The server isn't answering (timed out).", "サーバーから応答がありません（タイムアウト）。"]),
    ("err_cert", ["Certificat HTTPS refusé (auto-signé ou expiré ?).", "HTTPS certificate rejected (self-signed or expired?).", "HTTPS証明書が拒否されました（自己署名または期限切れ？）。"]),
    ("err_rejected", ["qBittorrent a refusé ce torrent (déjà présent ou invalide ?).", "qBittorrent turned this torrent down (already there or invalid?).", "qBittorrentがこのトレントを受け付けませんでした（登録済みか不正なファイルです）。"]),
    ("err_assoc", ["Association impossible sur ce système.", "File association isn't supported on this system.", "このシステムでは関連付けできません。"]),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_complete_and_used_keys_exist() {
        let mut seen = std::collections::HashSet::new();
        for (k, v) in T {
            assert!(seen.insert(*k), "clé en double : {k}");
            assert!(v.iter().all(|s| !s.is_empty()), "traduction vide : {k}");
        }
        for src in [include_str!("main.rs"), include_str!("api.rs")] {
            for part in src.split(".t(\"").skip(1) {
                let key = part.split('"').next().unwrap();
                assert!(seen.contains(key), "clé manquante : {key}");
            }
            for part in src.split("\"err_").skip(1) {
                let key = format!("err_{}", part.split('"').next().unwrap());
                assert!(seen.contains(key.as_str()), "clé manquante : {key}");
            }
        }
        assert_eq!(Lang::Ja.t("connect"), "接続");
        assert_eq!(Lang::En.msg("err_auth"), "Login rejected — double-check the username and password.");
        assert_eq!(Lang::Fr.msg("texte brut"), "texte brut");
    }
}
