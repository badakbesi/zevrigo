use std::path::PathBuf;

use zevrigo_network::Multiaddr;
use zevrigo_primitives::Network;

use super::{
    ArgError, Command, Config, REGTEST_SEEDS, USAGE, chain_path, default_p2p_port, dial_addresses,
    official_seeds, p2p_key_path, p2p_listen_address, p2p_ws_listen_address, parse_args,
};

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|arg| (*arg).to_owned()).collect()
}

fn run_config(datadir: &str, rpc_port: u16) -> Command {
    Command::Run(Config {
        network: Network::Regtest,
        datadir: PathBuf::from(datadir),
        rpc_port,
        p2p_port: 28_800,
        p2p_ws_port: None,
        connect: Vec::new(),
        official_seeds: false,
    })
}

#[test]
fn parses_help_version_and_run() {
    assert_eq!(parse_args(args(&[])), Ok(Command::Help));
    assert_eq!(parse_args(args(&["--help"])), Ok(Command::Help));
    assert_eq!(parse_args(args(&["-h"])), Ok(Command::Help));
    assert_eq!(parse_args(args(&["--version"])), Ok(Command::Version));
    assert_eq!(
        parse_args(args(&["--regtest", "--datadir", "/tmp/zvrg"])),
        Ok(run_config("/tmp/zvrg", 28_801))
    );
    assert_eq!(
        parse_args(args(&["--rpc-port", "0", "--datadir", "data", "--regtest"])),
        Ok(run_config("data", 0))
    );
    assert!(USAGE.contains("--datadir"));
    assert!(USAGE.contains("--regtest"));
    assert!(USAGE.contains("MOD-11"));
}

#[test]
fn rejects_bad_arguments() {
    let cases: [(&[&str], ArgError); 13] = [
        (&["--datadir", "x"], ArgError::MissingNetwork),
        (&["--regtest"], ArgError::MissingDatadir),
        (
            &["--testnet", "--regtest", "--datadir", "x"],
            ArgError::NetworkConflict,
        ),
        (&["--mainnet"], ArgError::NetworkUnavailable("mainnet")),
        (
            &["--regtest", "--datadir"],
            ArgError::MissingValue("--datadir"),
        ),
        (
            &["--regtest", "--datadir", "--rpc-port"],
            ArgError::MissingValue("--datadir"),
        ),
        (
            &["--regtest", "--datadir", "x", "--rpc-port", "65536"],
            ArgError::InvalidPort("65536".to_owned()),
        ),
        (
            &["--regtest", "--datadir", "x", "--rpc-port", "abc"],
            ArgError::InvalidPort("abc".to_owned()),
        ),
        (
            &["--regtest", "--regtest", "--datadir", "x"],
            ArgError::Duplicate("--regtest"),
        ),
        (
            &["--regtest", "--datadir", "x", "--datadir", "y"],
            ArgError::Duplicate("--datadir"),
        ),
        (
            &[
                "--regtest",
                "--datadir",
                "x",
                "--rpc-port",
                "1",
                "--rpc-port",
                "2",
            ],
            ArgError::Duplicate("--rpc-port"),
        ),
        (&["--regtest", "--help"], ArgError::Exclusive),
        (
            &["--regtest", "--datadir", "x", "--foo"],
            ArgError::Unknown("--foo".to_owned()),
        ),
    ];
    for (list, expected) in cases {
        assert_eq!(parse_args(args(list)), Err(expected), "{list:?}");
    }
}

#[test]
fn chain_file_lives_under_network_folder() {
    let Ok(Command::Run(config)) = parse_args(args(&["--regtest", "--datadir", "data"])) else {
        unreachable!("argumen sah");
    };
    assert_eq!(
        chain_path(&config),
        PathBuf::from("data").join("regtest").join("chain.redb")
    );
}

#[test]
fn parses_p2p_options() {
    let first = "/ip4/127.0.0.1/tcp/28800";
    let second = "/ip4/10.0.0.2/tcp/1/p2p/12D3KooWQK1wnefoLrcVHbbnf5tLzbopUd3K3bFAoJpA7YJgL5pV";
    let Ok(Command::Run(config)) = parse_args(args(&[
        "--regtest",
        "--datadir",
        "d",
        "--p2p-port",
        "0",
        "--connect",
        first,
        "--connect",
        second,
    ])) else {
        unreachable!("argumen sah");
    };
    assert_eq!(config.p2p_port, 0);
    assert_eq!(
        config.connect,
        vec![
            first.parse::<Multiaddr>().unwrap(),
            second.parse::<Multiaddr>().unwrap()
        ]
    );
    assert_eq!(
        p2p_key_path(&config),
        PathBuf::from("d").join("regtest").join("p2p_key")
    );
    assert_eq!(
        p2p_listen_address(28_800).unwrap(),
        first.parse::<Multiaddr>().unwrap()
    );
    assert_eq!(default_p2p_port(Network::Regtest), 28_800);
    assert_eq!(default_p2p_port(Network::Testnet), 18_800);
    assert_eq!(default_p2p_port(Network::Mainnet), 8_800);
    assert!(USAGE.contains("--p2p-port"));
    assert!(USAGE.contains("--connect"));

    let cases: [(&[&str], ArgError); 4] = [
        (
            &["--regtest", "--datadir", "x", "--p2p-port", "70000"],
            ArgError::InvalidP2pPort("70000".to_owned()),
        ),
        (
            &[
                "--regtest",
                "--datadir",
                "x",
                "--p2p-port",
                "1",
                "--p2p-port",
                "2",
            ],
            ArgError::Duplicate("--p2p-port"),
        ),
        (
            &[
                "--regtest",
                "--datadir",
                "x",
                "--connect",
                "127.0.0.1:28800",
            ],
            ArgError::InvalidAddress("127.0.0.1:28800".to_owned()),
        ),
        (
            &["--regtest", "--datadir", "x", "--connect"],
            ArgError::MissingValue("--connect"),
        ),
    ];
    for (list, expected) in cases {
        assert_eq!(parse_args(args(list)), Err(expected), "{list:?}");
    }
}

#[test]
fn parses_testnet() {
    let Ok(Command::Run(config)) = parse_args(args(&["--testnet", "--datadir", "d"])) else {
        unreachable!("argumen sah");
    };
    assert_eq!(
        config,
        Config {
            network: Network::Testnet,
            datadir: PathBuf::from("d"),
            rpc_port: 18_801,
            p2p_port: 18_800,
            p2p_ws_port: None,
            connect: Vec::new(),
            official_seeds: true,
        }
    );
    assert_eq!(
        chain_path(&config),
        PathBuf::from("d").join("testnet").join("chain.redb")
    );
    assert_eq!(
        p2p_key_path(&config),
        PathBuf::from("d").join("testnet").join("p2p_key")
    );
    assert_eq!(
        parse_args(args(&["--testnet", "--testnet", "--datadir", "d"])),
        Err(ArgError::Duplicate("--testnet"))
    );
    assert_eq!(
        parse_args(args(&["--regtest", "--testnet", "--datadir", "d"])),
        Err(ArgError::NetworkConflict)
    );
    assert!(USAGE.contains("--testnet"));
    assert_eq!(
        ArgError::MissingNetwork.to_string(),
        "jaringan wajib dipilih: --regtest atau --testnet"
    );
}

#[test]
fn parses_websocket_options() {
    let seed = concat!(
        "/dns4/demo-seed-p2p.zevrigo.online/tcp/443/tls/sni/demo-seed-p2p.zevrigo.online/ws",
        "/p2p/12D3KooWQK1wnefoLrcVHbbnf5tLzbopUd3K3bFAoJpA7YJgL5pV"
    );
    let Ok(Command::Run(config)) = parse_args(args(&[
        "--regtest",
        "--datadir",
        "d",
        "--p2p-ws-port",
        "28803",
        "--connect",
        seed,
    ])) else {
        unreachable!("argumen sah");
    };
    assert_eq!(config.p2p_ws_port, Some(28_803));
    assert_eq!(config.p2p_port, 28_800);
    assert_eq!(config.connect, vec![seed.parse::<Multiaddr>().unwrap()]);
    assert_eq!(
        p2p_ws_listen_address(28_803).unwrap(),
        "/ip4/127.0.0.1/tcp/28803/ws".parse::<Multiaddr>().unwrap()
    );
    assert!(USAGE.contains("--p2p-ws-port"));
    assert!(USAGE.contains("p2p-ws:"));
    assert!(USAGE.contains("/tls/sni/"));

    let Ok(Command::Run(plain)) = parse_args(args(&["--regtest", "--datadir", "d"])) else {
        unreachable!("argumen sah");
    };
    assert_eq!(plain.p2p_ws_port, None);

    let cases: [(&[&str], ArgError); 3] = [
        (
            &["--regtest", "--datadir", "x", "--p2p-ws-port", "65536"],
            ArgError::InvalidP2pWsPort("65536".to_owned()),
        ),
        (
            &[
                "--regtest",
                "--datadir",
                "x",
                "--p2p-ws-port",
                "1",
                "--p2p-ws-port",
                "2",
            ],
            ArgError::Duplicate("--p2p-ws-port"),
        ),
        (
            &["--regtest", "--datadir", "x", "--p2p-ws-port"],
            ArgError::MissingValue("--p2p-ws-port"),
        ),
    ];
    for (list, expected) in cases {
        assert_eq!(parse_args(args(list)), Err(expected), "{list:?}");
    }
}

#[test]
fn official_seeds_only_when_asked_on_regtest() {
    let seed = REGTEST_SEEDS.first().copied().unwrap_or_default();
    assert_eq!(
        seed,
        "/dns4/demo-seed-p2p.zevrigo.online/tcp/443/tls/sni/demo-seed-p2p.zevrigo.online/ws/p2p/12D3KooWABPBsu8gZWtL7bbeEPjg6gwRPDbp3GZidpTeUC95rRDo"
    );
    assert_eq!(official_seeds(Network::Regtest), REGTEST_SEEDS);
    assert!(official_seeds(Network::Testnet).is_empty());
    assert!(official_seeds(Network::Mainnet).is_empty());
    for network in [Network::Regtest, Network::Testnet, Network::Mainnet] {
        for text in official_seeds(network) {
            assert!(text.parse::<Multiaddr>().is_ok(), "{text}");
            assert!(text.contains("/p2p/12D3KooW"), "{text}");
        }
    }
    let seed_address = seed.parse::<Multiaddr>().unwrap();
    let parse = |list: &[&str]| match parse_args(args(list)) {
        Ok(Command::Run(config)) => config,
        other => unreachable!("{other:?}"),
    };

    let plain = parse(&["--regtest", "--datadir", "d"]);
    assert!(!plain.official_seeds);
    assert!(dial_addresses(&plain).unwrap().is_empty());

    let asked = parse(&["--regtest", "--datadir", "d", "--seed-resmi"]);
    assert!(asked.official_seeds);
    assert_eq!(dial_addresses(&asked).unwrap(), vec![seed_address.clone()]);

    let other = "/ip4/127.0.0.1/tcp/38800";
    let both = parse(&[
        "--regtest",
        "--datadir",
        "d",
        "--connect",
        other,
        "--seed-resmi",
        "--connect",
        seed,
    ]);
    assert_eq!(
        dial_addresses(&both).unwrap(),
        vec![other.parse::<Multiaddr>().unwrap(), seed_address]
    );

    let testnet = parse(&["--testnet", "--datadir", "d"]);
    assert!(testnet.official_seeds);
    assert!(dial_addresses(&testnet).unwrap().is_empty());
    let local = parse(&["--testnet", "--datadir", "d", "--tanpa-seed-resmi"]);
    assert!(!local.official_seeds);

    let cases: [(&[&str], ArgError); 3] = [
        (
            &[
                "--regtest",
                "--datadir",
                "x",
                "--seed-resmi",
                "--tanpa-seed-resmi",
            ],
            ArgError::SeedConflict,
        ),
        (
            &[
                "--regtest",
                "--datadir",
                "x",
                "--seed-resmi",
                "--seed-resmi",
            ],
            ArgError::Duplicate("--seed-resmi"),
        ),
        (
            &[
                "--testnet",
                "--datadir",
                "x",
                "--tanpa-seed-resmi",
                "--tanpa-seed-resmi",
            ],
            ArgError::Duplicate("--tanpa-seed-resmi"),
        ),
    ];
    for (list, expected) in cases {
        assert_eq!(parse_args(args(list)), Err(expected), "{list:?}");
    }
    assert!(USAGE.contains("--seed-resmi"));
    assert!(USAGE.contains("--tanpa-seed-resmi"));
    assert_eq!(
        ArgError::SeedConflict.to_string(),
        "--seed-resmi dan --tanpa-seed-resmi tidak boleh digabung"
    );
}
